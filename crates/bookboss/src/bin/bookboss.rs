#[cfg(feature = "server")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(not(feature = "server"))]
fn main() {
    bb_frontend::web::launch_web_frontend();
}

#[cfg(feature = "server")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use bookboss::{
        commands::{CommandLine, Commands},
        config::Config,
        logging::init_logging,
    };

    let cli: CommandLine = clap::Parser::parse();
    let config = Config::load().context("Cannot load configuration")?;

    match cli.command {
        Commands::DumpEpub { file } => cmd_dump_epub(file).await,
        Commands::OpenLibrary { isbn } => cmd_open_library(isbn).await,
        Commands::Hardcover { isbn } => cmd_hardcover(isbn, config).await,
        Commands::Grpc { host, port, command } => cmd_grpc(host, port, command).await,
        Commands::Server => {
            init_logging()?;
            cmd_server(config).await
        }
    }
}

#[cfg(feature = "server")]
async fn cmd_dump_epub(file: std::path::PathBuf) -> anyhow::Result<()> {
    use bb_core::{book::FileFormat, pipeline::MetadataExtractor};
    use bb_formats::{EpubExtractor, read_opf_metadata_xml};

    let raw = read_opf_metadata_xml(&file)?;
    println!("=== raw OPF metadata ===\n{raw}\n");

    let meta = EpubExtractor.extract(&file, FileFormat::Epub).await?;
    println!("=== extracted metadata ===");
    println!("title:        {:?}", meta.title);
    println!("authors:      {:?}", meta.authors);
    println!("description:  {:?}", meta.description);
    println!("publisher:    {:?}", meta.publisher);
    println!("published:    {:?}", meta.published_date);
    println!("language:     {:?}", meta.language);
    println!("identifiers:  {:?}", meta.identifiers);
    println!("series_name:  {:?}", meta.series_name);
    println!("series_num:   {:?}", meta.series_number);
    Ok(())
}

#[cfg(feature = "server")]
async fn cmd_open_library(isbn: String) -> anyhow::Result<()> {
    use bb_core::{
        book::IdentifierType,
        pipeline::{ExtractedIdentifier, ExtractedMetadata, MetadataProvider},
    };
    use bb_metadata::OpenLibraryAdapter;

    let isbn_type = if isbn.len() == 10 { IdentifierType::Isbn10 } else { IdentifierType::Isbn13 };
    let extracted = ExtractedMetadata {
        identifiers: Some(vec![ExtractedIdentifier {
            identifier_type: isbn_type,
            value: isbn.clone(),
        }]),
        ..Default::default()
    };

    let adapter = OpenLibraryAdapter::new();
    match adapter.enrich(&extracted).await? {
        None => println!("No record found on Open Library for ISBN {isbn}"),
        Some(book) => print_provider_book(&book),
    }
    Ok(())
}

#[cfg(feature = "server")]
async fn cmd_hardcover(isbn: String, config: bookboss::config::Config) -> anyhow::Result<()> {
    use bb_core::{
        book::IdentifierType,
        pipeline::{ExtractedIdentifier, ExtractedMetadata, MetadataProvider},
    };
    use bb_metadata::HardcoverAdapter;

    let Some(token) = config.metadata.hardcover_api_token else {
        anyhow::bail!("hardcover_api_token is not configured (set BOOKBOSS__METADATA__HARDCOVER_API_TOKEN)");
    };

    let isbn_type = if isbn.len() == 10 { IdentifierType::Isbn10 } else { IdentifierType::Isbn13 };
    let extracted = ExtractedMetadata {
        identifiers: Some(vec![ExtractedIdentifier {
            identifier_type: isbn_type,
            value: isbn.clone(),
        }]),
        ..Default::default()
    };

    let adapter = HardcoverAdapter::new(token);
    match adapter.enrich(&extracted).await? {
        None => println!("No record found on Hardcover for ISBN {isbn}"),
        Some(book) => print_provider_book(&book),
    }
    Ok(())
}

#[cfg(feature = "server")]
async fn cmd_grpc(host: String, port: u16, command: bookboss::commands::GrpcSubcommand) -> anyhow::Result<()> {
    use bb_api::grpc::system::api;
    use bookboss::commands::GrpcSubcommand;

    match command {
        GrpcSubcommand::Status => {
            let status = api::status(&host, port).await?;
            println!("Status: {status}");
        }
    }
    Ok(())
}

#[cfg(feature = "server")]
async fn cmd_server(config: bookboss::config::Config) -> anyhow::Result<()> {
    use std::{sync::Arc, time::Duration};

    use anyhow::Context;
    use bb_api::create_api_subsystem;
    use bb_core::{create_core_subsystem, create_services, jobs::JobRegistry, pipeline::PipelineServiceImpl};
    use bb_database::{create_repository_service, open_database};
    use bb_formats::{ConversionServiceImpl, EpubExtractor};
    use bb_frontend::server::create_frontend_subsystem;
    use bb_import::{ProcessImportHandler, create_import_subsystem};
    use bb_metadata::create_metadata_providers;
    use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle, Toplevel};

    tracing::info!("BookBoss {}", clap::crate_version!());
    let span = tracing::span!(tracing::Level::TRACE, "BookBoss Startup").entered();

    let database = open_database(&config.database).await.context("Couldn't create database connection")?;
    let repository_service = create_repository_service(database).await.context("Couldn't create database connection")?;
    let library_store = Arc::new(bb_storage::LocalLibraryStore::new(config.library.library_path.clone()));
    let pipeline_service = Arc::new(PipelineServiceImpl::new(
        repository_service.clone(),
        library_store.clone(),
        Arc::new(EpubExtractor),
        create_metadata_providers(&config.metadata),
    )) as Arc<dyn bb_core::pipeline::PipelineService>;
    let conversion_service = Arc::new(ConversionServiceImpl::new(repository_service.clone()));
    let core_services =
        create_services(repository_service.clone(), library_store, pipeline_service, conversion_service).context("Couldn't create core services")?;

    let scan_interval = Duration::from_secs(config.import.scan_interval_secs);
    let worker_poll_interval = Duration::from_secs(config.import.worker_poll_interval_secs);

    let mut registry = JobRegistry::new();
    registry.register(ProcessImportHandler::new(repository_service.clone(), core_services.pipeline_service.clone()));

    let api_subsystem = create_api_subsystem(&config.api, core_services.clone());
    let core_subsystem = create_core_subsystem(registry, repository_service.clone(), worker_poll_interval);
    let import_subsystem = create_import_subsystem(config.import.bookdrop_path.clone(), scan_interval, repository_service.clone());
    let frontend_subsystem = create_frontend_subsystem(&config.frontend, core_services.clone());

    span.exit();

    Toplevel::new(async |s: &mut SubsystemHandle| {
        s.start(SubsystemBuilder::new("Api", api_subsystem.into_subsystem()));
        s.start(SubsystemBuilder::new("Core", core_subsystem.into_subsystem()));
        s.start(SubsystemBuilder::new("Import", import_subsystem.into_subsystem()));
        s.start(SubsystemBuilder::new("Frontend", frontend_subsystem.into_subsystem()));
    })
    .catch_signals()
    .handle_shutdown_requests(Duration::from_secs(3))
    .await?;

    repository_service.repository().close().await.context("Couldn't close database")?;
    Ok(())
}

#[cfg(feature = "server")]
fn print_provider_book(book: &bb_core::pipeline::ProviderBook) {
    let m = &book.metadata;
    println!("title:        {:?}", m.title);
    println!("authors:      {:?}", m.authors);
    println!("description:  {:?}", m.description);
    println!("publisher:    {:?}", m.publisher);
    println!("published:    {:?}", m.published_date);
    println!("language:     {:?}", m.language);
    println!("identifiers:  {:?}", m.identifiers);
    println!("series_name:  {:?}", m.series_name);
    println!("series_num:   {:?}", m.series_number);
    println!("cover:        {}", if book.cover_bytes.is_some() { "found" } else { "not found" });
}
