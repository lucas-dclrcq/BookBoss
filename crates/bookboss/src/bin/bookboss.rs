#[cfg(feature = "server")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(not(feature = "server"))]
fn main() {
    bb_frontend::web::launch_web_frontend();
}

#[cfg(not(any(feature = "server", feature = "web")))]
fn main() {
    eprintln!("No feature selected. Build with --features server or --features web.");
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
        Commands::DumpBook { file } => cmd_dump_book(file).await,
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
async fn cmd_dump_book(file: std::path::PathBuf) -> anyhow::Result<()> {
    use bb_core::format::FormatService;

    let format_service = bb_formats::create_format_service();

    let (format, meta) = format_service.extract_metadata(&file).await?;
    println!("=== extracted metadata ({format:?}) ===");
    println!("title:        {:?}", meta.title);
    println!("authors:      {:?}", meta.authors);
    println!("description:  {:?}", meta.description);
    println!("publisher:    {:?}", meta.publisher);
    println!("published:    {:?}", meta.published_date);
    println!("language:     {:?}", meta.language);
    println!("identifiers:  {:?}", meta.identifiers);
    println!("series_name:  {:?}", meta.series_name);
    println!("series_num:   {:?}", meta.series_number);

    if let Ok(sidecar) = format_service.read_sidecar(&file).await {
        println!("\n=== bookboss sidecar fields ===");
        println!("genres:       {:?}", sidecar.genres);
        println!("tags:         {:?}", sidecar.tags);
        println!("page_count:   {:?}", sidecar.page_count);
        println!("status:       {:?}", sidecar.status);
        println!("series:       {:?}", sidecar.series);
        println!("metadata_src: {:?}", sidecar.metadata_source);
    }

    if format == bb_core::book::FileFormat::Epub {
        match tokio::task::spawn_blocking({
            let path = file.clone();
            move || bb_formats::epub::read_epub_opf_xml(&path)
        })
        .await
        {
            Ok(Ok(xml)) => {
                println!("\n=== OPF metadata ===");
                if let (Some(start), Some(end)) =
                    (xml.find("<metadata"), xml.find("</metadata>"))
                {
                    println!("{}", &xml[start..end + "</metadata>".len()]);
                } else {
                    println!("{xml}");
                }
            }
            Ok(Err(e)) => println!("\n=== raw OPF XML (error: {e}) ==="),
            Err(_) => {}
        }
    }

    Ok(())
}

#[cfg(feature = "server")]
async fn cmd_open_library(isbn: String) -> anyhow::Result<()> {
    use bb_core::{
        book::IdentifierType,
        metadata::MetadataProvider,
        pipeline::{ExtractedIdentifier, ExtractedMetadata},
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
        metadata::MetadataProvider,
        pipeline::{ExtractedIdentifier, ExtractedMetadata},
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
    use bb_core::{ExternalServicesBuilder, create_core_subsystem, create_services, format::FormatService};
    use bb_database::{create_repository_service, open_database};
    use bb_formats::create_format_service;
    use bb_frontend::server::create_frontend_subsystem;
    use bb_metadata::before_start as metadata_before_start;
    use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle, Toplevel};

    tracing::info!("BookBoss {}", clap::crate_version!());
    let span = tracing::span!(tracing::Level::TRACE, "BookBoss Startup").entered();

    let database = open_database(&config.database).await.context("Couldn't create database connection")?;
    let repository_service = create_repository_service(database).await.context("Couldn't create database connection")?;
    let file_store = Arc::new(bb_storage::LocalFileStore::new(config.library.library_path.clone()));
    let format_service: Arc<dyn FormatService> = Arc::new(create_format_service());
    let worker_poll_interval = Duration::from_secs(config.import.worker_poll_interval_secs);

    let external = ExternalServicesBuilder::default()
        .repository_service(repository_service.clone())
        .file_store(file_store)
        .format_service(format_service)
        .bookdrop_path(Some(config.import.bookdrop_path.clone()))
        .scan_interval(Some(Duration::from_secs(config.import.scan_interval_secs)))
        .build()
        .context("ExternalServices missing required field")?;
    let core_services = create_services(external, &config.encryption_secret).context("Couldn't create core services")?;

    // Each crate self-registers its job handlers and health task configs.
    bb_core::before_start(&core_services);
    // Register configured metadata providers into the metadata service.
    metadata_before_start(&core_services, &config.metadata);

    let api_subsystem = create_api_subsystem(&config.api, core_services.clone());
    let core_subsystem = create_core_subsystem(core_services.clone(), worker_poll_interval);
    let frontend_subsystem = create_frontend_subsystem(&config.frontend, core_services.clone());

    span.exit();

    Toplevel::new(async |s: &mut SubsystemHandle| {
        s.start(SubsystemBuilder::new("Api", api_subsystem.into_subsystem()));
        s.start(SubsystemBuilder::new("Core", core_subsystem.into_subsystem()));
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
