/// Lightweight signals broadcast to connected clients via SSE.
///
/// Each variant indicates that *something* changed — the frontend re-fetches
/// the relevant data through its normal server-function path.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// An import job reached `NeedsReview`, or was approved/rejected.
    IncomingChanged,
    /// A background job (enrich_epub / convert_kepub) was queued, completed,
    /// or failed.
    JobsChanged,
}
