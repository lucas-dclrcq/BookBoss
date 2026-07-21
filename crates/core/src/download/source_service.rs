use std::sync::{Arc, RwLock};

use crate::download::DownloadProvider;

/// Registry for the configured download provider.
///
/// Populated after `CoreServices` is built, from `bb_download::before_start`,
/// and only when the feature is enabled and an API key is configured. Mirrors
/// [`MetadataService`](crate::metadata::MetadataService) — interior mutability
/// so the registry can be filled after startup.
///
/// A `None` result from [`provider`](DownloadSourceService::provider) means the
/// feature is disabled/unconfigured; callers use this to gate the UI and reject
/// download requests.
pub trait DownloadSourceService: Send + Sync {
    /// Register a provider. The first registered provider is the active one.
    fn register(&self, provider: Arc<dyn DownloadProvider>);

    /// Return the active provider, or `None` if none is registered.
    fn provider(&self) -> Option<Arc<dyn DownloadProvider>>;
}

pub struct DownloadSourceServiceImpl {
    providers: RwLock<Vec<Arc<dyn DownloadProvider>>>,
}

impl DownloadSourceService for DownloadSourceServiceImpl {
    fn register(&self, provider: Arc<dyn DownloadProvider>) {
        self.providers.write().expect("download provider registry lock poisoned").push(provider);
    }

    fn provider(&self) -> Option<Arc<dyn DownloadProvider>> {
        self.providers.read().expect("download provider registry lock poisoned").first().cloned()
    }
}

#[must_use]
pub fn create_download_source_service() -> Arc<dyn DownloadSourceService> {
    Arc::new(DownloadSourceServiceImpl {
        providers: RwLock::new(vec![]),
    })
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::{
        Error,
        download::{DownloadCandidate, DownloadedFile},
    };

    struct DummyProvider;

    #[async_trait]
    impl DownloadProvider for DummyProvider {
        fn name(&self) -> &'static str {
            "Dummy"
        }
        async fn search(&self, _query: &str, _language: Option<&str>) -> Result<Vec<DownloadCandidate>, Error> {
            Ok(vec![])
        }
        async fn fetch(&self, _external_id: &str) -> Result<DownloadedFile, Error> {
            Ok(DownloadedFile {
                filename: "x.epub".into(),
                bytes: vec![],
            })
        }
    }

    #[test]
    fn provider_is_none_until_registered() {
        let svc = create_download_source_service();
        assert!(svc.provider().is_none());
        svc.register(Arc::new(DummyProvider));
        assert!(svc.provider().is_some());
        assert_eq!(svc.provider().unwrap().name(), "Dummy");
    }
}
