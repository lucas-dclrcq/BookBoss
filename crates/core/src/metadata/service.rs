use std::sync::{Arc, RwLock};

use crate::{
    Error,
    book::IdentifierType,
    metadata::MetadataProvider,
    pipeline::{
        ProviderBook,
        model::{ExtractedAuthor, ExtractedIdentifier, ExtractedMetadata},
    },
};

/// Port trait for managing the ordered list of configured metadata providers.
///
/// Providers are registered once at startup (via `bb_metadata::before_start`)
/// and queried by the pipeline at enrichment time. Uses interior mutability
/// so the registry can be populated after `CoreServices` is built — same
/// pattern as the job handler and health task registries.
#[async_trait::async_trait]
pub trait MetadataService: Send + Sync {
    /// Register a provider. Providers are returned in registration order.
    fn register(&self, provider: Arc<dyn MetadataProvider>);

    /// Return all registered providers in priority order.
    fn providers(&self) -> Vec<Arc<dyn MetadataProvider>>;

    /// Return the human-readable names of all registered providers, in
    /// priority order.
    fn list_provider_names(&self) -> Vec<&'static str>;

    /// Fetch metadata from a named provider using the supplied search context.
    ///
    /// Returns `None` when the provider finds no match or has insufficient
    /// data to query. Returns an error if the provider name is unknown.
    /// Cover bytes, if any, are included in the returned [`ProviderBook`].
    async fn fetch_from_provider(
        &self,
        provider_name: &str,
        title: Option<String>,
        authors: Vec<String>,
        identifiers: Vec<(IdentifierType, String)>,
    ) -> Result<Option<ProviderBook>, Error>;
}

pub struct MetadataServiceImpl {
    providers: RwLock<Vec<Arc<dyn MetadataProvider>>>,
}

#[async_trait::async_trait]
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

    async fn fetch_from_provider(
        &self,
        provider_name: &str,
        title: Option<String>,
        authors: Vec<String>,
        identifiers: Vec<(IdentifierType, String)>,
    ) -> Result<Option<ProviderBook>, Error> {
        let provider = self
            .providers()
            .into_iter()
            .find(|p| p.name() == provider_name)
            .ok_or_else(|| Error::Validation(format!("unknown provider: {provider_name}")))?;

        let extracted = ExtractedMetadata {
            title,
            authors: if authors.is_empty() {
                None
            } else {
                Some(
                    authors
                        .into_iter()
                        .enumerate()
                        .map(|(i, name)| ExtractedAuthor {
                            name,
                            role: None,
                            sort_order: i as i32,
                        })
                        .collect(),
                )
            },
            identifiers: if identifiers.is_empty() {
                None
            } else {
                Some(
                    identifiers
                        .into_iter()
                        .map(|(identifier_type, value)| ExtractedIdentifier { identifier_type, value })
                        .collect(),
                )
            },
            ..Default::default()
        };

        provider.enrich(&extracted).await
    }
}

#[must_use]
pub fn create_metadata_service() -> Arc<dyn MetadataService> {
    Arc::new(MetadataServiceImpl {
        providers: RwLock::new(vec![]),
    })
}
