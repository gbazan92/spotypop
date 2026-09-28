use std::fmt;

use serde::Deserialize;

use crate::fl;

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
        let text = match self {
            Self::SignedOut => fl!("error-signed-out"),
            Self::Reauth => fl!("error-reauth"),
            Self::NoClientId => fl!("error-no-client-id"),
            Self::NoDevice => fl!("error-no-device"),
            Self::PremiumRequired => fl!("error-premium"),
            Self::RateLimited { retry_after } => {
                fl!("error-rate-limited", seconds = (*retry_after).max(1))
            }
            Self::Forbidden(message) => fl!("error-forbidden", message = message.as_str()),
            Self::NotFound(message) => fl!("error-not-found", message = message.as_str()),
            Self::Server(message) => fl!("error-server", message = message.as_str()),
            Self::Http { status, message } => {
                fl!(
                    "error-http",
                    status = status.to_string(),
                    message = message.as_str()
                )
            }
            Self::Network(message) => fl!("error-network", message = message.as_str()),
            Self::TooLarge => fl!("error-too-large"),
            Self::BadArgument(message) => fl!("error-bad-argument", message = message.as_str()),
            Self::PortBusy(port) => fl!("error-port-busy", port = port.to_string()),
            Self::LoginTimeout => fl!("error-login-timeout"),
            Self::LoginDenied(reason) => fl!("error-login-denied", reason = reason.as_str()),
            Self::Browser(message) => fl!("error-browser", message = message.as_str()),
            Self::Storage(message) => fl!("error-storage", message = message.as_str()),
        };
        f.write_str(&text)
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
            403 => Self::Forbidden(or("access denied")),
            404 if reason == "NO_ACTIVE_DEVICE" || lower.contains("device") => Self::NoDevice,
            404 => Self::NotFound(or("no such resource")),
            429 => Self::RateLimited {
                retry_after: retry_after.unwrap_or(1),
            },
            500..=599 => Self::Server(or(&format!("HTTP {status}"))),
            _ => Self::Http {
                status,
                message: or("request failed"),
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
