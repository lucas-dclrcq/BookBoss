pub mod model;
pub mod service;

pub use model::{ExtractedAuthor, ExtractedIdentifier, ExtractedMetadata, ProviderBook};
pub use service::{PipelineService, PipelineServiceImpl};
