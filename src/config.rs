use cosmic::cosmic_config::{
    self, Config, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry,
};
use serde::{Deserialize, Serialize};

pub const DEFAULT_REDIRECT_PORT: u16 = 8888;

/// What the panel shows, besides the plain icon, while something is playing.
/// Every variant stays inside the bar's height.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelLook {
    #[default]
    Cover,
    #[serde(alias = "mirror", alias = "dots")]
    Bars,
    #[serde(alias = "trail")]
    Wave,
    Fill,
}

impl PanelLook {
    pub const ALL: [Self; 4] = [Self::Cover, Self::Bars, Self::Wave, Self::Fill];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cover => "Portada y título",
            Self::Bars => "Barras",
            Self::Wave => "Ondas",
            Self::Fill => "Relleno",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|look| *look == self).unwrap_or(0)
    }
}

#[derive(Clone, CosmicConfigEntry, Debug, Eq, PartialEq)]
#[version = 1]
pub struct AppConfig {
    /// Client ID of the user's own app at developer.spotify.com; not a secret.
    pub client_id: String,
    /// Must match the redirect URI registered in that app.
    pub redirect_port: u16,
    pub show_track: bool,
    pub panel_look: PanelLook,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            redirect_port: DEFAULT_REDIRECT_PORT,
            show_track: true,
            panel_look: PanelLook::Cover,
        }
    }
}

impl AppConfig {
    pub fn redirect_uri(&self) -> String {
        // Spotify rejects `localhost`; only the loopback literal is accepted.
        format!("http://127.0.0.1:{}/callback", self.redirect_port)
    }
}

pub fn load(app_id: &str) -> (Option<Config>, AppConfig) {
    let handler = match Config::new(app_id, AppConfig::VERSION) {
        Ok(handler) => handler,
        Err(error) => {
            eprintln!("unable to open config {app_id}: {error}");
            return (None, AppConfig::default());
        }
    };

    let config = AppConfig::get_entry(&handler).unwrap_or_else(|(errors, config)| {
        for error in errors.into_iter().filter(is_real_error) {
            eprintln!("unable to read config key: {error}");
        }
        config
    });

    (Some(handler), config)
}

/// A key that was never written just means "use the default".
fn is_real_error(error: &cosmic_config::Error) -> bool {
    match error {
        cosmic_config::Error::GetKey(_, io) => io.kind() != std::io::ErrorKind::NotFound,
        other => other.is_err(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_looks_have_distinct_labels() {
        let mut labels: Vec<_> = PanelLook::ALL.iter().map(|look| look.label()).collect();
        let count = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), count);
        assert_eq!(PanelLook::Cover.index(), 0);
    }

    #[test]
    fn old_look_names_still_load() {
        assert_eq!(
            serde_json::from_str::<PanelLook>("\"dots\"").unwrap(),
            PanelLook::Bars
        );
        assert_eq!(
            serde_json::from_str::<PanelLook>("\"trail\"").unwrap(),
            PanelLook::Wave
        );
        assert_eq!(
            serde_json::from_str::<PanelLook>("\"wave\"").unwrap(),
            PanelLook::Wave
        );
    }
}
