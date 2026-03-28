use std::sync::{Arc, RwLock};

use crate::metadata::MetadataProvider;

/// Port trait for managing the ordered list of configured metadata providers.
///
/// Providers are registered once at startup (via `bb_metadata::before_start`)
/// and queried by the pipeline at enrichment time. Uses interior mutability
/// so the registry can be populated after `CoreServices` is built — same
/// pattern as the job handler and health task registries.
pub trait MetadataService: Send + Sync {
    /// Register a provider. Providers are returned in registration order.
    fn register(&self, provider: Arc<dyn MetadataProvider>);

    /// Return all registered providers in priority order.
    fn providers(&self) -> Vec<Arc<dyn MetadataProvider>>;

    /// Return the human-readable names of all registered providers, in
    /// priority order.
    fn list_provider_names(&self) -> Vec<&'static str>;
}

pub struct MetadataServiceImpl {
    providers: RwLock<Vec<Arc<dyn MetadataProvider>>>,
}

impl MetadataService for MetadataServiceImpl {
    fn register(&self, provider: Arc<dyn MetadataProvider>) {
        self.providers.write().expect("metadata provider registry lock poisoned").push(provider);
    }

    fn providers(&self) -> Vec<Arc<dyn MetadataProvider>> {
        self.providers.read().expect("metadata provider registry lock poisoned").clone()
    }

    fn list_provider_names(&self) -> Vec<&'static str> {
        self.providers
            .read()
            .expect("metadata provider registry lock poisoned")
            .iter()
            .map(|p| p.name())
            .collect()
    }
}

#[must_use]
pub fn create_metadata_service() -> Arc<dyn MetadataService> {
    Arc::new(MetadataServiceImpl {
        providers: RwLock::new(vec![]),
    })
}
