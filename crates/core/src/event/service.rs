use tokio::sync::broadcast;

use super::AppEvent;

/// Port for broadcasting real-time application events.
///
/// Services call these methods to notify connected clients (via SSE) that
/// something changed. The trait hides the broadcast channel so callers don't
/// need to know the transport mechanism.
pub trait EventService: Send + Sync {
    /// An import job reached `NeedsReview`, or was approved/rejected.
    fn notify_incoming_changed(&self);

    /// A background job was queued, completed, or failed.
    fn notify_jobs_changed(&self);

    /// Subscribe to the event stream. Each subscriber gets its own receiver.
    fn subscribe(&self) -> broadcast::Receiver<AppEvent>;
}

pub struct EventServiceImpl {
    sender: broadcast::Sender<AppEvent>,
}

impl EventServiceImpl {
    #[must_use]
    pub fn new(sender: broadcast::Sender<AppEvent>) -> Self {
        Self { sender }
    }
}

impl EventService for EventServiceImpl {
    fn notify_incoming_changed(&self) {
        tracing::debug!("broadcasting IncomingChanged event");
        let _ = self.sender.send(AppEvent::IncomingChanged);
    }

    fn notify_jobs_changed(&self) {
        tracing::debug!("broadcasting JobsChanged event");
        let _ = self.sender.send(AppEvent::JobsChanged);
    }

    fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.sender.subscribe()
    }
}
