use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Instant, timeout, timeout_at};
use url::Url;

use super::http::{self, Http};
use super::{Error, StoredAuth};
use crate::fl;

const ACCOUNTS: &str = "https://accounts.spotify.com";

pub const SCOPES: &[&str] = &[
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-currently-playing",
    "user-read-recently-played",
    "user-library-read",
    "user-library-modify",
    "playlist-read-private",
    "playlist-read-collaborative",
];

const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
/// Per connection, so a silent peer cannot hold the listener.
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
/// Request line and headers together.
const MAX_REQUEST_BYTES: usize = 16 * 1024;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::fill(buffer.as_mut_slice());
    URL_SAFE_NO_PAD.encode(buffer)
}

pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn authorize_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> Url {
    let mut url = Url::parse(ACCOUNTS).expect("static URL");
    url.set_path("/authorize");
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", challenge)
        .append_pair("state", state)
        .append_pair("scope", &SCOPES.join(" "));
    url
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[serde(default = "default_expiry")]
    pub expires_in: u64,
    #[serde(default)]
    pub scope: String,
}

fn default_expiry() -> u64 {
    3600
}

impl TokenResponse {
    pub fn apply(self, auth: &mut StoredAuth) {
        auth.access_token = self.access_token;
        if let Some(refresh) = self.refresh_token.filter(|token| !token.is_empty()) {
            auth.refresh_token = refresh;
        }
        auth.expires_at = now() + self.expires_in;
        if !self.scope.is_empty() {
            auth.scope = self.scope;
        }
        auth.needs_reauth = false;
    }
}

#[derive(Deserialize)]
struct TokenError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
}

enum TokenFailure {
    /// The refresh token or client is no longer accepted.
    Rejected(String),
    Other(Error),
}

async fn token_request(http: &Http, form: &[(&str, &str)]) -> Result<TokenResponse, TokenFailure> {
    let response = http
        .client()
        .post(format!("{ACCOUNTS}/api/token"))
        .form(form)
        .send()
        .await
        .map_err(|error| TokenFailure::Other(error.into()))?;
    let status = response.status().as_u16();
    let ok = response.status().is_success();
    let body = http::read_capped(
        response,
        if ok {
            http::API_MAX_BYTES
        } else {
            http::ERROR_MAX_BYTES
        },
    )
    .await
    .map_err(TokenFailure::Other)?;

    if ok {
        return serde_json::from_slice(&body)
            .map_err(|error| TokenFailure::Other(Error::Network(error.to_string())));
    }
    let parsed: Option<TokenError> = serde_json::from_slice(&body).ok();
    let (code, description) = parsed.map_or_else(
        || (String::new(), format!("HTTP {status}")),
        |parsed| (parsed.error, parsed.error_description),
    );
    if matches!(
        code.as_str(),
        "invalid_grant" | "invalid_client" | "unauthorized_client"
    ) {
        Err(TokenFailure::Rejected(if description.is_empty() {
            code
        } else {
            description
        }))
    } else {
        Err(TokenFailure::Other(Error::Http {
            status,
            message: if description.is_empty() {
                code
            } else {
                description
            },
        }))
    }
}

pub async fn refresh(http: &Http, auth: &StoredAuth) -> Result<TokenResponse, Error> {
    if auth.refresh_token.is_empty() {
        return Err(Error::SignedOut);
    }
    token_request(
        http,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &auth.refresh_token),
            ("client_id", &auth.client_id),
        ],
    )
    .await
    .map_err(|failure| match failure {
        TokenFailure::Rejected(_) => Error::Reauth,
        TokenFailure::Other(error) => error,
    })
}

/// A login whose callback listener is already bound, waiting for the browser.
pub struct PendingLogin {
    listener: std::net::TcpListener,
    client_id: String,
    redirect_uri: String,
    verifier: String,
    state: String,
    url: Url,
}

impl PendingLogin {
    /// The page to open in the browser.
    pub fn url(&self) -> &Url {
        &self.url
    }
}

/// Binds the callback listener before the browser is opened, so a busy port
/// is reported right away instead of after the user approves.
pub fn begin_login(client_id: &str, port: u16) -> Result<PendingLogin, Error> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err(Error::NoClientId);
    }
    if port < 1024 {
        return Err(Error::BadArgument(
            "el puerto debe estar entre 1024 y 65535".into(),
        ));
    }
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|_| Error::PortBusy(port))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| Error::Network(error.to_string()))?;

    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let verifier = random_token(64);
    let state = random_token(16);
    let url = authorize_url(client_id, &redirect_uri, &challenge_for(&verifier), &state);
    Ok(PendingLogin {
        listener,
        client_id: client_id.to_owned(),
        redirect_uri,
        verifier,
        state,
        url,
    })
}

/// Waits for the browser to come back and returns a fresh session (without the user profile).
pub async fn finish_login(http: &Http, pending: PendingLogin) -> Result<StoredAuth, Error> {
    let listener = TcpListener::from_std(pending.listener)
        .map_err(|error| Error::Network(error.to_string()))?;
    let code = wait_for_code(&listener, &pending.state).await?;
    drop(listener);

    let tokens = token_request(
        http,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &pending.redirect_uri),
            ("client_id", &pending.client_id),
            ("code_verifier", &pending.verifier),
        ],
    )
    .await
    .map_err(|failure| match failure {
        TokenFailure::Rejected(reason) => Error::LoginDenied(reason),
        TokenFailure::Other(error) => error,
    })?;

    let mut auth = StoredAuth {
        client_id: pending.client_id,
        authorized_at: now(),
        ..StoredAuth::default()
    };
    tokens.apply(&mut auth);
    if auth.refresh_token.is_empty() {
        return Err(Error::LoginDenied(
            "Spotify returned no refresh token".into(),
        ));
    }
    Ok(auth)
}

async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, Error> {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        let (stream, _) = timeout_at(deadline, listener.accept())
            .await
            .map_err(|_| Error::LoginTimeout)?
            .map_err(|error| Error::Network(error.to_string()))?;
        // A broken or slow connection is dropped; the login keeps waiting.
        if let Ok(Some(outcome)) = timeout(SOCKET_TIMEOUT, serve(stream, state)).await {
            return outcome;
        }
    }
}

/// Answers one browser request. `None` means "not our callback, keep waiting".
async fn serve(mut stream: TcpStream, state: &str) -> Option<Result<String, Error>> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while !buffer.windows(2).any(|pair| pair == b"\r\n") && buffer.len() < MAX_REQUEST_BYTES {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read.min(MAX_REQUEST_BYTES - buffer.len())]);
    }
    let line = String::from_utf8_lossy(&buffer);
    let target = line
        .lines()
        .next()
        .and_then(|first| first.strip_prefix("GET "))
        .and_then(|rest| rest.split(' ').next())
        .unwrap_or_default();

    let (status, body, outcome) = match parse_callback(target, state) {
        Callback::NotFound => ("404 Not Found", fl!("login-page-not-found"), None),
        Callback::Ignored => ("200 OK", fl!("login-page-ignored"), None),
        Callback::Denied(reason) => (
            "200 OK",
            fl!("login-page-denied", reason = escape_html(&reason)),
            Some(Err(Error::LoginDenied(reason))),
        ),
        Callback::Malformed => (
            "200 OK",
            fl!("login-page-malformed"),
            Some(Err(Error::LoginDenied("malformed code".into()))),
        ),
        Callback::Code(code) => ("200 OK", fl!("login-page-done"), Some(Ok(code))),
    };

    let page = format!(
        "<!doctype html><meta charset=utf-8><title>SpotyPop</title>\
         <body style=\"font-family:sans-serif;background:#1b1b1b;color:#e0e0e0;display:flex;\
         align-items:center;justify-content:center;height:100vh;margin:0\">\
         <div style=\"text-align:center\"><div style=\"font-size:48px\">&#9835;</div><p>{body}</p></div></body>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
    outcome
}

#[derive(Debug, PartialEq, Eq)]
enum Callback {
    NotFound,
    /// Any web page can point the browser here; without our `state` the
    /// request neither completes nor cancels the login.
    Ignored,
    Denied(String),
    Malformed,
    Code(String),
}

fn parse_callback(target: &str, expected_state: &str) -> Callback {
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Callback::NotFound;
    };
    if url.path() != "/callback" {
        return Callback::NotFound;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    let state = param("state").unwrap_or_default();
    if !constant_time_eq(state.as_bytes(), expected_state.as_bytes()) {
        return Callback::Ignored;
    }
    if let Some(error) = param("error") {
        let reason: String = error
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ' ' | '.' | '-'))
            .take(80)
            .collect();
        return Callback::Denied(reason);
    }
    match param("code") {
        Some(code)
            if !code.is_empty()
                && code.len() <= 2048
                && code
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') =>
        {
            Callback::Code(code)
        }
        _ => Callback::Malformed,
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

fn escape_html(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '&' => "&amp;".to_owned(),
            '<' => "&lt;".to_owned(),
            '>' => "&gt;".to_owned(),
            '"' => "&quot;".to_owned(),
            '\'' => "&#39;".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc_7636() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifier_has_valid_length_and_alphabet() {
        let verifier = random_token(64);
        assert!((43..=128).contains(&verifier.len()));
        assert!(
            verifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        assert_ne!(verifier, random_token(64));
    }

    #[test]
    fn authorize_url_carries_pkce_and_scopes() {
        let url = authorize_url("cid", "http://127.0.0.1:8888/callback", "chal", "st");
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        let get = |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(url.host_str(), Some("accounts.spotify.com"));
        assert_eq!(get("code_challenge_method"), Some("S256"));
        assert_eq!(get("redirect_uri"), Some("http://127.0.0.1:8888/callback"));
        assert!(get("scope").unwrap().contains("user-modify-playback-state"));
    }

    #[test]
    fn callback_requires_matching_state() {
        assert_eq!(
            parse_callback("/callback?code=abc&state=nope", "st"),
            Callback::Ignored
        );
        assert_eq!(
            parse_callback("/callback?code=abc", "st"),
            Callback::Ignored
        );
        assert_eq!(
            parse_callback("/callback?code=abc-_1&state=st", "st"),
            Callback::Code("abc-_1".into())
        );
    }

    #[test]
    fn callback_reports_denial_sanitized() {
        assert_eq!(
            parse_callback("/callback?error=access_denied%3Cscript%3E&state=st", "st"),
            Callback::Denied("access_deniedscript".into())
        );
    }

    #[test]
    fn callback_rejects_other_paths_and_bad_codes() {
        assert_eq!(parse_callback("/favicon.ico", "st"), Callback::NotFound);
        assert_eq!(parse_callback("", "st"), Callback::NotFound);
        assert_eq!(
            parse_callback("/callback?code=a%20b&state=st", "st"),
            Callback::Malformed
        );
    }

    #[test]
    fn escapes_html() {
        assert_eq!(
            escape_html("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }

    #[tokio::test]
    async fn listener_serves_callback_end_to_end() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let browser = tokio::spawn(async move {
            // A stray request first: must be ignored, not end the login.
            for target in [
                "/favicon.ico",
                "/callback?code=zzz&state=wrong",
                "/callback?code=good&state=st",
            ] {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                stream
                    .write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut reply = String::new();
                stream.read_to_string(&mut reply).await.unwrap();
                assert!(reply.starts_with("HTTP/1.1 "));
            }
        });

        assert_eq!(wait_for_code(&listener, "st").await, Ok("good".into()));
        browser.await.unwrap();
    }

    #[test]
    fn begin_login_reports_a_busy_port_up_front() {
        let taken = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = taken.local_addr().unwrap().port();
        assert_eq!(begin_login("cid", port).err(), Some(Error::PortBusy(port)));
        assert_eq!(begin_login("  ", 8888).err(), Some(Error::NoClientId));
    }
}
