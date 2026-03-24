pub mod conversion;
pub mod enrich_handler;
pub mod epub;
pub mod epub_enrich;
mod error;
pub mod kepub_convert;
pub mod kepub_handler;
pub mod opf;

pub use conversion::{ConversionServiceImpl, ConvertKepubPayload, EnrichEpubPayload};
pub use enrich_handler::EnrichEpubHandler;
pub use epub::{EpubExtractor, read_opf_metadata_xml, read_opf_xml};
pub use epub_enrich::enrich_epub;
pub use error::Error;
pub use kepub_handler::ConvertKepubHandler;
pub use opf::parse_sidecar;

/// Register format-related job handlers.
///
/// Called once after `CoreServices` is built.
pub fn before_start(core: &std::sync::Arc<bb_core::CoreServices>) {
    use bb_core::jobs::JobServiceExt;

    core.job_service.register(EnrichEpubHandler::new(core.clone()));
    core.job_service.register(ConvertKepubHandler::new(core.clone()));
}
