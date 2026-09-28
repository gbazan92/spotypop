//! Local Spotify Connect receiver for `SpotyPop`.
//!
//! `login` authorizes this computer once and stores a reusable librespot
//! credential; `run` registers a Connect device that plays through
//! PulseAudio/PipeWire. The applet drives it through the Web API like any other
//! device, so the music keeps going when the panel restarts. While it plays,
//! the panel scopes follow the actual audio through a local socket.

mod paths;
mod scope;
mod sink;

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use librespot_connect::{ConnectConfig, Spirc};
use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::config::{DeviceType, SessionConfig};
use librespot_core::session::Session;
use librespot_playback::config::{Bitrate, PlayerConfig};
use librespot_playback::mixer::{self, Mixer, MixerConfig};
use librespot_playback::player::Player;
use sha2::{Digest, Sha256};

use crate::sink::{PulseTap, Tap};

/// librespot's own default; any loopback port is accepted by Spotify's client.
const DEFAULT_OAUTH_PORT: u16 = 5588;
const INITIAL_VOLUME_PERCENT: u32 = 70;
const AUDIO_CACHE_BYTES: u64 = 1_000_000_000;
const RECONNECT_LIMIT: usize = 5;
const RECONNECT_WINDOW: Duration = Duration::from_mins(10);

const OAUTH_SCOPES: &[&str] = &[
    "streaming",
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-currently-playing",
    "user-read-private",
    "user-read-email",
    "playlist-read-private",
    "playlist-read-collaborative",
    "user-library-read",
];

/// Exit codes the applet relies on.
mod exit {
    pub const USAGE: u8 = 2;
    pub const NEEDS_LOGIN: u8 = 3;
    pub const FAILED: u8 = 1;
}

#[derive(Debug)]
enum Failure {
    Usage(String),
    NeedsLogin,
    Other(String),
}

impl Failure {
    fn code(&self) -> u8 {
        match self {
            Self::Usage(_) => exit::USAGE,
            Self::NeedsLogin => exit::NEEDS_LOGIN,
            Self::Other(_) => exit::FAILED,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(message) | Self::Other(message) => f.write_str(message),
            Self::NeedsLogin => f.write_str("this computer is not authorized for playback yet"),
        }
    }
}

fn other(context: &str) -> impl FnOnce(librespot_core::Error) -> Failure + '_ {
    move |error| Failure::Other(format!("{context}: {error}"))
}

enum Action {
    Run { name: String },
    Login { port: u16 },
}

fn parse_args() -> Result<Action, Failure> {
    let mut args = std::env::args().skip(1);
    let usage = || {
        Failure::Usage(
            "usage: spotypop-player run --name <device name> | login [--port <port>]".into(),
        )
    };
    let command = args.next().ok_or_else(usage)?;
    let mut name = None;
    let mut port = DEFAULT_OAUTH_PORT;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(usage)?;
        match flag.as_str() {
            "--name" => name = Some(value),
            "--port" => port = value.parse().map_err(|_| usage())?,
            _ => return Err(usage()),
        }
    }
    match command.as_str() {
        "run" => {
            let name = name.unwrap_or_else(|| "COSMIC".into());
            let name = name.trim().to_owned();
            if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
                return Err(Failure::Usage(
                    "the device name must be 1 to 64 printable characters".into(),
                ));
            }
            Ok(Action::Run { name })
        }
        "login" => Ok(Action::Login { port }),
        _ => Err(usage()),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let result = match parse_args() {
        Ok(Action::Login { port }) => login(port).await,
        Ok(Action::Run { name }) => run(name).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("{failure}");
            ExitCode::from(failure.code())
        }
    }
}

fn private_dir(path: &Path) -> Result<(), Failure> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|error| Failure::Other(format!("cannot create {}: {error}", path.display())))
}

fn credentials_cache() -> Result<Cache, Failure> {
    let dir = paths::credentials_dir();
    private_dir(&dir)?;
    Cache::new(Some(dir.as_path()), None, None, None).map_err(other("credential store"))
}

/// Credentials plus up to [`AUDIO_CACHE_BYTES`] of recently played audio.
fn audio_cache() -> Result<Cache, Failure> {
    let audio_dir = paths::audio_cache_dir();
    private_dir(&audio_dir)?;
    Cache::new(
        Some(paths::credentials_dir().as_path()),
        Some(audio_dir.as_path()),
        Some(audio_dir.as_path()),
        Some(AUDIO_CACHE_BYTES),
    )
    .map_err(other("audio cache"))
}

async fn login(port: u16) -> Result<(), Failure> {
    let cache = credentials_cache()?;
    let session_config = SessionConfig::default();
    let client = librespot_oauth::OAuthClientBuilder::new(
        &session_config.client_id,
        &format!("http://127.0.0.1:{port}/login"),
        OAUTH_SCOPES.to_vec(),
    )
    .with_custom_message(
        "<h2>Done, this computer can now play Spotify.</h2><p>You can close this tab.</p>",
    )
    .open_in_browser()
    .build()
    .map_err(|error| Failure::Other(format!("oauth client: {error}")))?;

    let token = client
        .get_access_token_async()
        .await
        .map_err(|error| Failure::Other(format!("authorization did not complete: {error}")))?;

    // Connecting once with `store_credentials` swaps the short-lived token for
    // a reusable credential in the cache.
    let session = Session::new(session_config, Some(cache));
    session
        .connect(Credentials::with_access_token(token.access_token), true)
        .await
        .map_err(other("could not save the playback login"))?;
    session.shutdown();
    println!("playback authorized");
    Ok(())
}

/// Held for the life of the receiver so a second `run` exits at once.
struct Instance {
    _file: File,
}

fn claim_instance() -> Result<Option<Instance>, Failure> {
    let path = paths::pid_file();
    if let Some(dir) = path.parent() {
        private_dir(dir)?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .map_err(|error| Failure::Other(format!("cannot open {}: {error}", path.display())))?;
    // SAFETY: flock only reads the descriptor, which `file` keeps open.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Ok(None);
    }
    file.set_len(0)
        .and_then(|()| write!(file, "{}", std::process::id()))
        .map_err(|error| Failure::Other(format!("cannot write {}: {error}", path.display())))?;
    Ok(Some(Instance { _file: file }))
}

fn device_id(name: &str) -> String {
    Sha256::digest(name.as_bytes()).iter().take(20).fold(
        String::with_capacity(40),
        |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        },
    )
}

async fn run(name: String) -> Result<(), Failure> {
    let Some(_instance) = claim_instance()? else {
        log::info!("another receiver is already running");
        return Ok(());
    };

    let credentials = credentials_cache()?
        .credentials()
        .ok_or(Failure::NeedsLogin)?;

    let cache = audio_cache()?;
    let session_config = SessionConfig {
        device_id: device_id(&name),
        ap_port: Some(443),
        autoplay: Some(true),
        tmp_dir: std::env::temp_dir(),
        ..SessionConfig::default()
    };
    let player_config = PlayerConfig {
        bitrate: Bitrate::Bitrate320,
        gapless: true,
        position_update_interval: Some(Duration::from_secs(1)),
        ..PlayerConfig::default()
    };

    let mixer = mixer()?;
    let tap = Arc::new(Tap::default());
    let scope = tokio::spawn(scope::serve(Arc::clone(&tap)));
    let mut session = Session::new(session_config.clone(), Some(cache.clone()));
    let player = Player::new(
        player_config,
        session.clone(),
        mixer.get_soft_volume(),
        move || Box::new(PulseTap::new(tap)),
    );

    #[allow(clippy::cast_possible_truncation)]
    let initial_volume = (u32::from(u16::MAX) * INITIAL_VOLUME_PERCENT / 100) as u16;
    let connect_config = ConnectConfig {
        name,
        device_type: DeviceType::Computer,
        initial_volume,
        ..ConnectConfig::default()
    };

    let (mut spirc, task) = Spirc::new(
        connect_config.clone(),
        session.clone(),
        credentials.clone(),
        Arc::clone(&player),
        Arc::clone(&mixer),
    )
    .await
    .map_err(|error| {
        if error.to_string().to_lowercase().contains("credentials") {
            Failure::NeedsLogin
        } else {
            Failure::Other(format!("could not reach Spotify: {error}"))
        }
    })?;
    let mut task = tokio::spawn(task);
    log::info!("receiver ready");

    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|error| Failure::Other(format!("signal handler: {error}")))?;
    let mut reconnects = VecDeque::new();

    loop {
        tokio::select! {
            _ = terminate.recv() => break,
            _ = tokio::signal::ctrl_c() => break,
            result = &mut task => {
                if let Err(error) = result {
                    log::warn!("session task failed: {error}");
                }
            }
        }

        if !allow_reconnect(&mut reconnects, Instant::now()) {
            return Err(Failure::Other("the Spotify session keeps dropping".into()));
        }
        log::warn!("session ended, reconnecting");
        if !session.is_invalid() {
            session.shutdown();
        }
        session = Session::new(session_config.clone(), Some(cache.clone()));
        player.set_session(session.clone());
        let (next, next_task) = Spirc::new(
            connect_config.clone(),
            session.clone(),
            credentials.clone(),
            Arc::clone(&player),
            Arc::clone(&mixer),
        )
        .await
        .map_err(other("could not reconnect"))?;
        spirc = next;
        task = tokio::spawn(next_task);
        log::info!("reconnected");
    }

    let _ = spirc.shutdown();
    let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
    scope.abort();
    let _ = fs::remove_file(paths::scope_socket());
    Ok(())
}

/// librespot's own software volume.
fn mixer() -> Result<Arc<dyn Mixer>, Failure> {
    mixer::find(Some("softvol"))
        .ok_or_else(|| Failure::Other("the software mixer is not compiled in".into()))?(
        MixerConfig::default(),
    )
    .map_err(other("mixer"))
}

fn allow_reconnect(attempts: &mut VecDeque<Instant>, now: Instant) -> bool {
    while attempts
        .front()
        .is_some_and(|attempt| now.saturating_duration_since(*attempt) >= RECONNECT_WINDOW)
    {
        attempts.pop_front();
    }
    if attempts.len() >= RECONNECT_LIMIT {
        return false;
    }
    attempts.push_back(now);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids_are_stable_hex() {
        let id = device_id("pop-os (COSMIC)");
        assert_eq!(id.len(), 40);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(id, device_id("pop-os (COSMIC)"));
        assert_ne!(id, device_id("other"));
    }

    #[test]
    fn reconnects_are_rate_limited() {
        let start = Instant::now();
        let mut attempts = VecDeque::new();
        for _ in 0..RECONNECT_LIMIT {
            assert!(allow_reconnect(&mut attempts, start));
        }
        assert!(!allow_reconnect(&mut attempts, start));
        assert!(allow_reconnect(&mut attempts, start + RECONNECT_WINDOW));
    }
}
