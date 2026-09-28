use std::fmt;

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    SignedOut,
    Reauth,
    NoClientId,
    NoDevice,
    PremiumRequired,
    RateLimited { retry_after: u64 },
    Forbidden(String),
    NotFound(String),
    Server(String),
    Http { status: u16, message: String },
    Network(String),
    TooLarge,
    BadArgument(String),
    PortBusy(u16),
    LoginTimeout,
    LoginDenied(String),
    Browser(String),
    Storage(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SignedOut => f.write_str("No estás conectado a Spotify."),
            Self::Reauth => f.write_str("Tu sesión de Spotify venció. Conectate de nuevo."),
            Self::NoClientId => f.write_str("Falta el Client ID de tu app de Spotify."),
            Self::NoDevice => f.write_str("No hay ningún dispositivo de Spotify activo."),
            Self::PremiumRequired => {
                f.write_str("Spotify Premium es necesario para controlar la reproducción.")
            }
            Self::RateLimited { retry_after } => write!(
                f,
                "Demasiadas solicitudes a Spotify. Reintentá en {}s.",
                (*retry_after).max(1)
            ),
            Self::Forbidden(message) => write!(f, "Spotify rechazó la solicitud: {message}"),
            Self::NotFound(message) => write!(f, "No encontrado: {message}"),
            Self::Server(message) => write!(f, "Error del servidor de Spotify: {message}"),
            Self::Http { status, message } => {
                write!(f, "Spotify respondió HTTP {status}: {message}")
            }
            Self::Network(message) => write!(f, "No se pudo contactar a Spotify: {message}"),
            Self::TooLarge => f.write_str("Spotify envió una respuesta demasiado grande."),
            Self::BadArgument(message) => write!(f, "Dato inválido: {message}"),
            Self::PortBusy(port) => write!(
                f,
                "El puerto {port} está ocupado. Elegí otro puerto de redirección."
            ),
            Self::LoginTimeout => {
                f.write_str("Se agotó el tiempo esperando la respuesta de Spotify.")
            }
            Self::LoginDenied(reason) => write!(f, "El inicio de sesión falló: {reason}"),
            Self::Browser(message) => write!(f, "No se pudo abrir el navegador: {message}"),
            Self::Storage(message) => write!(f, "No se pudo guardar la sesión: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        // Without the URL: it could carry query parameters.
        Self::Network(error.without_url().to_string())
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: Option<ErrorField>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorField {
    Detailed {
        #[serde(default)]
        message: String,
        #[serde(default)]
        reason: String,
    },
    Plain(String),
}

impl Error {
    /// Maps a failed Web API reply to an error, the way the Omarchy bridge did.
    pub fn from_response(status: u16, body: &[u8], retry_after: Option<u64>) -> Self {
        let (message, reason) = match serde_json::from_slice::<ErrorBody>(body)
            .ok()
            .and_then(|parsed| parsed.error)
        {
            Some(ErrorField::Detailed { message, reason }) => (message, reason),
            Some(ErrorField::Plain(message)) => (message, String::new()),
            None => (String::new(), String::new()),
        };
        let lower = message.to_lowercase();
        let or = |fallback: &str| {
            if message.is_empty() {
                fallback.to_owned()
            } else {
                message.clone()
            }
        };

        match status {
            401 => Self::Reauth,
            403 if reason == "PREMIUM_REQUIRED" || lower.contains("premium") => {
                Self::PremiumRequired
            }
            403 => Self::Forbidden(or("acceso denegado")),
            404 if reason == "NO_ACTIVE_DEVICE" || lower.contains("device") => Self::NoDevice,
            404 => Self::NotFound(or("recurso inexistente")),
            429 => Self::RateLimited {
                retry_after: retry_after.unwrap_or(1),
            },
            500..=599 => Self::Server(or(&format!("HTTP {status}"))),
            _ => Self::Http {
                status,
                message: or("solicitud fallida"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_premium_required() {
        let body = br#"{"error":{"status":403,"message":"Player command failed: Premium required","reason":"PREMIUM_REQUIRED"}}"#;
        assert_eq!(
            Error::from_response(403, body, None),
            Error::PremiumRequired
        );
    }

    #[test]
    fn maps_no_active_device() {
        let body = br#"{"error":{"status":404,"message":"Player command failed: No active device found","reason":"NO_ACTIVE_DEVICE"}}"#;
        assert_eq!(Error::from_response(404, body, None), Error::NoDevice);
    }

    #[test]
    fn maps_rate_limit_with_retry_after() {
        assert_eq!(
            Error::from_response(429, b"", Some(7)),
            Error::RateLimited { retry_after: 7 }
        );
    }

    #[test]
    fn keeps_plain_messages() {
        let body = br#"{"error":"something odd"}"#;
        assert_eq!(
            Error::from_response(400, body, None),
            Error::Http {
                status: 400,
                message: "something odd".into()
            }
        );
    }

    #[test]
    fn tolerates_non_json_bodies() {
        assert_eq!(
            Error::from_response(502, b"<html>bad gateway</html>", None),
            Error::Server("HTTP 502".into())
        );
    }
}
