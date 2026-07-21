use crate::types::Capabilities;

#[derive(Debug, Clone)]
pub struct AppSetting {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct NewAppSetting {
    pub key: String,
    pub value: String,
}

/// Default parameters applied to accounts auto-provisioned on first OIDC login.
///
/// Persisted as three separate `oidc.provisioning.*` app-setting rows (see
/// [`AppSettingService::oidc_provisioning_defaults`]). Library tokens are kept
/// as strings and round-tripped verbatim — the caller parses them into
/// `LibraryToken` when assigning libraries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OidcProvisioningDefaults {
    /// Capabilities granted to the provisioned user. The role is expressed via
    /// `Capability::Admin`; `Capability::SuperAdmin` is never allowed here.
    pub capabilities: Capabilities,
    /// Library tokens to assign to the provisioned user.
    pub library_tokens: Vec<String>,
    /// Default library token — must be one of `library_tokens`. `None` falls
    /// back to the All Books library at resolution time.
    pub default_library: Option<String>,
}
