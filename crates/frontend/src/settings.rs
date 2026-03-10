/// Frontend-specific setting keys for the user setting store.
///
/// Keys use the `frontend:` namespace to avoid collisions with other adapters.
#[cfg(feature = "server")]
pub(crate) enum FrontendSettings {
    ApiKey,
}

#[cfg(feature = "server")]
impl FrontendSettings {
    pub(crate) fn key(&self) -> &'static str {
        match self {
            Self::ApiKey => "frontend:api_key",
        }
    }
}

#[cfg(test)]
#[cfg(feature = "server")]
mod tests {
    use super::FrontendSettings;

    #[test]
    fn frontend_settings_key_is_namespaced() {
        assert_eq!(FrontendSettings::ApiKey.key(), "frontend:api_key");
    }
}
