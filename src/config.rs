use cosmic::cosmic_config::{
    self, Config, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry,
};

pub const DEFAULT_REDIRECT_PORT: u16 = 8888;

#[derive(Clone, CosmicConfigEntry, Debug, Eq, PartialEq)]
#[version = 1]
pub struct AppConfig {
    /// Client ID of the user's own app at developer.spotify.com; not a secret.
    pub client_id: String,
    /// Must match the redirect URI registered in that app.
    pub redirect_port: u16,
    pub show_track: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            redirect_port: DEFAULT_REDIRECT_PORT,
            show_track: true,
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
