pub mod model;
pub mod service;

use std::sync::Arc;

pub use model::AppEvent;
pub use service::EventService;

use self::service::EventServiceImpl;

/// Creates an `EventService` backed by a `tokio::sync::broadcast` channel.
#[must_use]
pub fn create_event_service(capacity: usize) -> Arc<dyn EventService> {
    let (sender, _) = tokio::sync::broadcast::channel(capacity);
    Arc::new(EventServiceImpl::new(sender))
}
