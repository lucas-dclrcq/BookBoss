use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    Extension,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use bb_core::{CoreServices, event::AppEvent};
use tokio_stream::{Stream, StreamExt, wrappers::BroadcastStream};

use super::AuthSession;

/// Debounce window — duplicate events of the same type within this window are
/// collapsed into a single SSE message.
const DEBOUNCE: Duration = Duration::from_millis(500);

/// SSE endpoint that streams real-time application events to the browser.
///
/// Requires an authenticated session. Each connected client receives its own
/// broadcast receiver; rapid-fire events of the same type are debounced so the
/// frontend doesn't trigger excessive re-fetches.
pub(crate) async fn event_stream(
    Extension(auth_session): Extension<AuthSession>,
    Extension(core_services): Extension<Arc<CoreServices>>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    if auth_session.current_user.as_ref().is_none_or(|u| u.username.is_empty()) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let rx = core_services.event_service.subscribe();
    let stream = debounced_event_stream(rx);

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30)).text("ping")))
}

/// Wraps a broadcast receiver into a debounced SSE stream.
///
/// When an event arrives, we wait up to [`DEBOUNCE`] for more events.  All
/// events received during the window are deduplicated by type, then emitted as
/// individual SSE messages. This collapses (e.g.) 20 rapid `IncomingChanged`
/// broadcasts into a single SSE event.
fn debounced_event_stream(rx: tokio::sync::broadcast::Receiver<AppEvent>) -> impl Stream<Item = Result<Event, Infallible>> {
    let inner = BroadcastStream::new(rx);

    async_stream::stream! {
        tokio::pin!(inner);

        loop {
            // Wait for the first event (blocks until something arrives).
            let first = loop {
                match inner.next().await {
                    Some(Ok(ev)) => break ev,
                    Some(Err(_)) => {}  // lagged — skip
                    None => return,     // channel closed
                }
            };

            let mut has_incoming = matches!(first, AppEvent::IncomingChanged);
            let mut has_jobs = matches!(first, AppEvent::JobsChanged);
            let mut has_messages = matches!(first, AppEvent::SystemMessagesChanged);

            // Collect any further events that arrive within the debounce window.
            let deadline = tokio::time::Instant::now() + DEBOUNCE;
            loop {
                tokio::select! {
                    maybe = inner.next() => {
                        match maybe {
                            Some(Ok(AppEvent::IncomingChanged)) => has_incoming = true,
                            Some(Ok(AppEvent::JobsChanged)) => has_jobs = true,
                            Some(Ok(AppEvent::SystemMessagesChanged)) => has_messages = true,
                            Some(Err(_)) => {} // lagged
                            None => {
                                // Channel closed — emit whatever we have, then return.
                                if has_incoming {
                                    yield Ok(Event::default().event("incoming_changed").data("updated"));
                                }
                                if has_jobs {
                                    yield Ok(Event::default().event("jobs_changed").data("updated"));
                                }
                                if has_messages {
                                    yield Ok(Event::default().event("system_messages_changed").data("updated"));
                                }
                                return;
                            }
                        }
                    }
                    () = tokio::time::sleep_until(deadline) => break,
                }
            }

            if has_incoming {
                yield Ok(Event::default().event("incoming_changed").data("updated"));
            }
            if has_jobs {
                yield Ok(Event::default().event("jobs_changed").data("updated"));
            }
            if has_messages {
                yield Ok(Event::default().event("system_messages_changed").data("updated"));
            }
        }
    }
}
