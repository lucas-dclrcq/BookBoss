pub mod model;
pub mod store;

pub use model::{BookSidecar, SidecarAuthor, SidecarFile, SidecarIdentifier, SidecarSeries};
pub use store::FileStoreService;
#[cfg(any(test, feature = "test-support"))]
pub use store::MockFileStoreService;
