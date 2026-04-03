mod model;
mod repository;
mod service;
pub use model::BookEdit;
pub use repository::CollectionRepository;
#[cfg(test)]
pub use repository::MockCollectionRepository;
#[cfg(any(test, feature = "test-support"))]
pub use service::MockCollectionService;
pub use service::{CollectionService, CollectionServiceImpl, CollectionStats};
