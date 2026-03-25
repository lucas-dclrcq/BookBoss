/// Deprecated: will be removed once all callers migrate to `FormatService`.
pub mod extractor;
pub mod model;
pub mod provider;
pub mod service;

pub use model::{BookEdit, ExtractedAuthor, ExtractedIdentifier, ExtractedMetadata, ProviderBook};
pub use provider::MetadataProvider;
pub use service::{PipelineService, PipelineServiceImpl};
