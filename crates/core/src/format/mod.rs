mod model;
mod service;

pub use model::{EBookFile, EnrichmentRequest};
pub use service::FormatService;
#[cfg(any(test, feature = "test-support"))]
pub use service::MockFormatService;
