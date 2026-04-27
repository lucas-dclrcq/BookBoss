use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub(crate) fn cycle(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub(crate) fn from_str(s: &str) -> Self {
        match s {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::System => "⊙",
            Self::Light => "☀",
            Self::Dark => "🌙",
        }
    }
}

pub(crate) static THEME_MODE: GlobalSignal<ThemeMode> = Signal::global(ThemeMode::default);

// ── Server functions
// ──────────────────────────────────────────────────────────

#[cfg(feature = "server")]
use {
    crate::routes::server_helpers::{authenticated_user, to_server_err},
    crate::server::AuthSession,
    bb_core::CoreServices,
    std::sync::Arc,
};

const THEME_SETTING_KEY: &str = "ui_theme";

#[get(
    "/api/v1/settings/theme",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn get_theme_preference() -> Result<Option<ThemeMode>, ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();
    let setting = core_services
        .user_setting_service
        .get(user_id, THEME_SETTING_KEY)
        .await
        .map_err(to_server_err)?;
    Ok(setting.map(|s| ThemeMode::from_str(&s.value)))
}

#[post(
    "/api/v1/settings/theme",
    auth_session: axum::Extension<AuthSession>,
    core_services: axum::Extension<Arc<CoreServices>>
)]
pub(crate) async fn set_theme_preference(mode: ThemeMode) -> Result<(), ServerFnError> {
    let user_id = authenticated_user(&auth_session)?.id();
    core_services
        .user_setting_service
        .set(user_id, THEME_SETTING_KEY, mode.as_str())
        .await
        .map_err(to_server_err)?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_rotates_system_light_dark_system() {
        assert_eq!(ThemeMode::System.cycle(), ThemeMode::Light);
        assert_eq!(ThemeMode::Light.cycle(), ThemeMode::Dark);
        assert_eq!(ThemeMode::Dark.cycle(), ThemeMode::System);
    }

    #[test]
    fn as_str_from_str_round_trip() {
        for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
            assert_eq!(ThemeMode::from_str(mode.as_str()), mode);
        }
    }

    #[test]
    fn from_str_unknown_defaults_to_system() {
        assert_eq!(ThemeMode::from_str("bogus"), ThemeMode::System);
        assert_eq!(ThemeMode::from_str(""), ThemeMode::System);
    }

    #[test]
    fn icon_returns_distinct_values() {
        let icons = [ThemeMode::System.icon(), ThemeMode::Light.icon(), ThemeMode::Dark.icon()];
        assert_eq!(icons.iter().collect::<std::collections::HashSet<_>>().len(), 3);
    }
}
