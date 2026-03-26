pub mod model;
pub mod provider;
pub mod service;

pub use model::{ExtractedAuthor, ExtractedIdentifier, ExtractedMetadata, ProviderBook};
pub use provider::MetadataProvider;
pub use service::{PipelineService, PipelineServiceImpl};
