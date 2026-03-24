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
    use bb_formats::{EpubExtractor, parse_sidecar, read_opf_metadata_xml, read_opf_xml};

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

    let opf_xml = read_opf_xml(&file)?;
    if let Ok(sidecar) = parse_sidecar(opf_xml.as_bytes()) {
        println!("\n=== bookboss sidecar fields ===");
        println!("genres:       {:?}", sidecar.genres);
        println!("tags:         {:?}", sidecar.tags);
        println!("page_count:   {:?}", sidecar.page_count);
        println!("status:       {:?}", sidecar.status);
        println!("series:       {:?}", sidecar.series);
        println!("metadata_src: {:?}", sidecar.metadata_source);
    }
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
    use bb_core::{
        ExternalServicesBuilder, create_core_subsystem, create_services,
        health::{
            create_health_service, create_health_subsystem, default_health_tasks,
            handlers::{
                cleanup_expired_sessions::CleanupExpiredSessionsHandler, cleanup_old_import_jobs::CleanupOldImportJobsHandler,
                cleanup_old_jobs::CleanupOldJobsHandler, cleanup_old_system_messages::CleanupOldSystemMessagesHandler,
                cleanup_orphan_authors::CleanupOrphanAuthorsHandler, cleanup_orphan_publishers::CleanupOrphanPublishersHandler,
                cleanup_orphan_series::CleanupOrphanSeriesHandler, ensure_enrichments::EnsureEnrichmentsHandler,
                recover_enrichments::RecoverEnrichmentsHandler, reset_stale_import_jobs::ResetStaleImportJobsHandler,
                verify_file_integrity::VerifyFileIntegrityHandler,
            },
        },
        jobs::{JobServiceExt, create_job_service},
        pipeline::PipelineServiceImpl,
    };
    use bb_database::{create_repository_service, open_database};
    use bb_formats::{ConversionServiceImpl, ConvertKepubHandler, EnrichEpubHandler, EpubExtractor};
    use bb_frontend::server::create_frontend_subsystem;
    use bb_import::{ProcessImportHandler, create_import_subsystem, create_scan_trigger};
    use bb_metadata::create_metadata_providers;
    use tokio_graceful_shutdown::{IntoSubsystem, SubsystemBuilder, SubsystemHandle, Toplevel};

    tracing::info!("BookBoss {}", clap::crate_version!());
    let span = tracing::span!(tracing::Level::TRACE, "BookBoss Startup").entered();

    let database = open_database(&config.database).await.context("Couldn't create database connection")?;
    let repository_service = create_repository_service(database).await.context("Couldn't create database connection")?;
    let file_store = Arc::new(bb_storage::LocalFileStore::new(config.library.library_path.clone()));
    let job_service = create_job_service(repository_service.clone());
    let conversion_service = Arc::new(ConversionServiceImpl::new(job_service.clone()));
    let event_service = bb_core::event::create_event_service(64);

    let pipeline_service = Arc::new(PipelineServiceImpl::new(
        repository_service.clone(),
        file_store.clone(),
        Arc::new(EpubExtractor),
        create_metadata_providers(&config.metadata),
        conversion_service.clone(),
        event_service.clone(),
    )) as Arc<dyn bb_core::pipeline::PipelineService>;

    let scan_interval = Duration::from_secs(config.import.scan_interval_secs);
    let worker_poll_interval = Duration::from_secs(config.import.worker_poll_interval_secs);

    // Create channels before CoreServices so triggers can be passed in via
    // ExternalServices, while receivers go to their respective subsystems.
    let (scan_trigger, scan_receiver) = create_scan_trigger();
    let (health_service, health_kick_rx) = create_health_service();
    for config in default_health_tasks() {
        health_service.register_task(config);
    }
    let external = ExternalServicesBuilder::default()
        .repository_service(repository_service.clone())
        .file_store(file_store)
        .pipeline_service(pipeline_service)
        .conversion_service(conversion_service)
        .job_service(job_service)
        .health_service(health_service)
        .import_scanner(Arc::new(scan_trigger.clone()))
        .event_service(event_service.clone())
        .build()
        .context("ExternalServices missing required field")?;
    let core_services = create_services(external, &config.encryption_secret).context("Couldn't create core services")?;

    let import_subsystem = create_import_subsystem(
        config.import.bookdrop_path.clone(),
        scan_interval,
        scan_trigger,
        scan_receiver,
        core_services.import_job_service.clone(),
    );

    // Register job handlers via JobService
    let js = &core_services.job_service;
    js.register(ProcessImportHandler::new(
        core_services.import_job_service.clone(),
        core_services.pipeline_service.clone(),
    ));
    js.register(EnrichEpubHandler::new(repository_service.clone(), core_services.file_store.clone()));
    js.register(ConvertKepubHandler::new(repository_service.clone(), core_services.file_store.clone()));

    // Health check handlers
    let sms = core_services.system_message_service.clone();
    js.register(RecoverEnrichmentsHandler::new(repository_service.clone(), sms.clone()));
    js.register(EnsureEnrichmentsHandler::new(repository_service.clone(), sms.clone()));
    js.register(CleanupOrphanAuthorsHandler::new(repository_service.clone(), sms.clone()));
    js.register(CleanupOrphanSeriesHandler::new(repository_service.clone(), sms.clone()));
    js.register(CleanupOrphanPublishersHandler::new(repository_service.clone(), sms.clone()));
    js.register(CleanupOldJobsHandler::new(repository_service.clone(), sms.clone()));
    js.register(CleanupOldImportJobsHandler::new(repository_service.clone(), sms.clone()));
    js.register(CleanupOldSystemMessagesHandler::new(sms.clone()));
    js.register(CleanupExpiredSessionsHandler::new(core_services.auth_service.clone()));
    js.register(VerifyFileIntegrityHandler::new(
        repository_service.clone(),
        sms.clone(),
        core_services.file_store.clone(),
    ));
    js.register(ResetStaleImportJobsHandler::new(repository_service.clone(), sms));

    let health_subsystem = create_health_subsystem(
        core_services.health_service.clone(),
        repository_service.repository().clone(),
        repository_service.job_repository().clone(),
        event_service.clone(),
        health_kick_rx,
    );

    let api_subsystem = create_api_subsystem(&config.api, core_services.clone());
    let core_subsystem = create_core_subsystem(
        core_services.job_service.clone(),
        repository_service.clone(),
        worker_poll_interval,
        event_service,
    );
    let frontend_subsystem = create_frontend_subsystem(&config.frontend, core_services.clone());

    span.exit();

    Toplevel::new(async |s: &mut SubsystemHandle| {
        s.start(SubsystemBuilder::new("Api", api_subsystem.into_subsystem()));
        s.start(SubsystemBuilder::new("Core", core_subsystem.into_subsystem()));
        s.start(SubsystemBuilder::new("Import", import_subsystem.into_subsystem()));
        s.start(SubsystemBuilder::new("Health", health_subsystem.into_subsystem()));
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
