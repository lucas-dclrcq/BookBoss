use std::collections::HashMap;

use crate::jobs::handler::{ErasedJobHandler, JobHandler};

pub struct JobRegistry {
    handlers: HashMap<String, Box<dyn ErasedJobHandler>>,
}

impl JobRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self { handlers: HashMap::new() }
    }

    pub fn register<H: JobHandler>(&mut self, handler: H) -> &mut Self {
        self.handlers.insert(H::JOB_TYPE.to_string(), Box::new(handler));
        self
    }

    pub(crate) fn get(&self, job_type: &str) -> Option<&dyn ErasedJobHandler> {
        self.handlers.get(job_type).map(std::convert::AsRef::as_ref)
    }
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    struct AlphaHandler;

    impl JobHandler for AlphaHandler {
        const JOB_TYPE: &'static str = "alpha.job";
        type Payload = serde_json::Value;

        async fn handle(&self, _payload: Self::Payload) -> Result<(), Error> {
            Ok(())
        }
    }

    struct BetaHandler;

    impl JobHandler for BetaHandler {
        const JOB_TYPE: &'static str = "beta.job";
        type Payload = serde_json::Value;

        async fn handle(&self, _payload: Self::Payload) -> Result<(), Error> {
            Ok(())
        }
    }

    #[test]
    fn new_registry_is_empty() {
        let registry = JobRegistry::new();
        assert!(registry.get("alpha.job").is_none());
    }

    #[test]
    fn default_creates_empty_registry() {
        let registry = JobRegistry::default();
        assert!(registry.get("anything").is_none());
    }

    #[test]
    fn register_and_get_returns_handler() {
        let mut registry = JobRegistry::new();
        registry.register(AlphaHandler);
        assert!(registry.get("alpha.job").is_some());
    }

    #[test]
    fn get_unknown_type_returns_none() {
        let mut registry = JobRegistry::new();
        registry.register(AlphaHandler);
        assert!(registry.get("unknown.type").is_none());
    }

    #[test]
    fn register_second_handler_overwrites_first_of_same_type() {
        let mut registry = JobRegistry::new();
        registry.register(AlphaHandler);
        registry.register(AlphaHandler);
        assert!(registry.get("alpha.job").is_some());
    }

    #[test]
    fn register_multiple_distinct_handlers() {
        let mut registry = JobRegistry::new();
        registry.register(AlphaHandler);
        registry.register(BetaHandler);
        assert!(registry.get("alpha.job").is_some());
        assert!(registry.get("beta.job").is_some());
        assert!(registry.get("gamma.job").is_none());
    }
}
