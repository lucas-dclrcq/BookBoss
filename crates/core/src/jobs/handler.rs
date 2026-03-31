use std::pin::Pin;

use serde::de::DeserializeOwned;

use crate::Error;

/// Implement this trait on your handler struct. `JOB_TYPE` must match the
/// corresponding `Enqueueable::JOB_TYPE` on the payload.
pub trait JobHandler: Send + Sync + 'static {
    const JOB_TYPE: &'static str;
    const DISPLAY_NAME: &'static str;
    type Payload: DeserializeOwned + Send;

    fn handle(&self, payload: Self::Payload) -> impl Future<Output = Result<(), Error>> + Send;
}

/// Object-safe erased version of `JobHandler` used for dynamic dispatch.
///
/// Handler authors implement [`JobHandler`] — this trait exists for type-erased
/// storage in the job registry. The blanket impl below bridges the two.
pub trait ErasedJobHandler: Send + Sync {
    /// The job type string this handler is registered for.
    fn job_type(&self) -> &str;
    /// Human-readable display name for this handler.
    fn display_name(&self) -> &str;
    fn handle<'a>(&'a self, payload: serde_json::Value) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;
}

impl<H: JobHandler> ErasedJobHandler for H {
    fn job_type(&self) -> &str {
        H::JOB_TYPE
    }

    fn display_name(&self) -> &str {
        H::DISPLAY_NAME
    }

    fn handle<'a>(&'a self, payload: serde_json::Value) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            let typed: H::Payload = serde_json::from_value(payload).map_err(|e| Error::Infrastructure(format!("job payload deserialize failed: {e}")))?;
            JobHandler::handle(self, typed).await
        })
    }
}
