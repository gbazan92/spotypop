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

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_core::Stream;
use librespot_connect::{ConnectConfig, Spirc};
use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::config::{DeviceType, SessionConfig};
use librespot_core::dealer::Subscription;
use librespot_core::session::Session;
use librespot_playback::config::{Bitrate, PlayerConfig};
use librespot_playback::mixer::{self, Mixer, MixerConfig};
use librespot_playback::player::Player;
use sha2::{Digest, Sha256};
use tokio::signal::unix::{Signal, SignalKind};
use tokio::task::JoinHandle;
use tokio::time::{Interval, MissedTickBehavior};

use crate::sink::{PulseTap, Tap};

/// librespot's own default; any loopback port is accepted by Spotify's client.
const DEFAULT_OAUTH_PORT: u16 = 5588;
const INITIAL_VOLUME_PERCENT: u32 = 70;
const AUDIO_CACHE_BYTES: u64 = 1_000_000_000;
/// librespot invalidates a silent session after its 80 s keep-alive, but the
/// Connect task keeps waiting on the dealer and never ends on its own.
const HEALTH_INTERVAL: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// A session that lasted this long resets the retry delay.
const STABLE_AFTER: Duration = Duration::from_mins(1);
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_mins(1);
/// librespot retries a lost dealer websocket every 10 s, but gives up for good,
/// without a word, when it cannot fetch a token for the new one. Past this,
/// the device is gone from Spotify and the session is rebuilt instead.
const DEALER_GRACE: Duration = Duration::from_secs(40);
/// Spotify pushes a new connection id every time the dealer reconnects.
const CONNECTION_ID_URI: &str = "hm://pusher/v1/connections/";
/// What librespot logs when the dealer's websocket drops or fails to reopen.
const DEALER_LOSS_SIGNS: &[&str] = &["Websocket", "Error while connecting", "Dealer finished"];

/// Unix time of the latest dealer loss. librespot reports it only in its log.
static DEALER_TROUBLE: AtomicU64 = AtomicU64::new(0);

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
    Run { name: String, log: Option<PathBuf> },
    Login { port: u16 },
}

fn parse_args() -> Result<Action, Failure> {
    let mut args = std::env::args().skip(1);
    let usage = || {
        Failure::Usage(
            "usage: spotypop-player run --name <device name> [--log <file>] | login [--port <port>]"
                .into(),
        )
    };
    let command = args.next().ok_or_else(usage)?;
    let mut name = None;
    let mut log = None;
    let mut port = DEFAULT_OAUTH_PORT;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(usage)?;
        match flag.as_str() {
            "--name" => name = Some(value),
            "--log" => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err(usage());
                }
                log = Some(path);
            }
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
            Ok(Action::Run { name, log })
        }
        "login" => Ok(Action::Login { port }),
        _ => Err(usage()),
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// `YYYY-MM-DD HH:MM:SS` in local time, without pulling in a date crate.
fn local_time() -> String {
    let now = unix_seconds();
    let Ok(secs) = libc::time_t::try_from(now) else {
        return now.to_string();
    };
    // SAFETY: an all-zero `tm` is valid, and localtime_r only writes into it.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&raw const secs, &raw mut tm) }.is_null() {
        return now.to_string();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

fn init_logging() {
    let logger =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .format(|buf, record| {
                writeln!(
                    buf,
                    "{} {:<5} {}: {}",
                    local_time(),
                    record.level(),
                    record.target(),
                    record.args()
                )
            })
            .build();
    log::set_max_level(logger.filter());
    let _ = log::set_boxed_logger(Box::new(DealerWatch(logger)));
}

/// Passes every record on, noting when the dealer reports a lost connection.
struct DealerWatch(env_logger::Logger);

impl log::Log for DealerWatch {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        self.0.enabled(metadata)
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.level() <= log::Level::Warn
            && record.target() == "librespot_core::dealer"
            && is_dealer_loss(&record.args().to_string())
        {
            DEALER_TROUBLE.store(unix_seconds(), Ordering::Relaxed);
        }
        self.0.log(record);
    }

    fn flush(&self) {
        self.0.flush();
    }
}

fn is_dealer_loss(message: &str) -> bool {
    DEALER_LOSS_SIGNS.iter().any(|sign| message.contains(sign))
}

/// Points stderr, and with it the logger, at `path`, keeping the previous run
/// as `<path>.1`. Only the instance holding the lock may do this, or a second
/// launch would wipe the log of the one that is actually playing.
fn redirect_stderr(path: &Path) -> Result<(), Failure> {
    if let Some(dir) = path.parent() {
        private_dir(dir)?;
    }
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() > 0) {
        let mut previous = path.as_os_str().to_owned();
        previous.push(".1");
        let _ = fs::rename(path, previous);
    }
    let file = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| Failure::Other(format!("cannot open {}: {error}", path.display())))?;
    // SAFETY: both descriptors are open; dup2 swaps stderr in one step.
    if unsafe { libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO) } < 0 {
        return Err(Failure::Other(format!(
            "cannot log to {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    init_logging();
    let result = match parse_args() {
        Ok(Action::Login { port }) => login(port).await,
        Ok(Action::Run { name, log }) => Box::pin(run(name, log)).await,
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

async fn run(name: String, log: Option<PathBuf>) -> Result<(), Failure> {
    let Some(_instance) = claim_instance()? else {
        log::info!("another receiver is already running");
        return Ok(());
    };
    if let Some(path) = log {
        redirect_stderr(&path)?;
    }

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

    let mut terminate = tokio::signal::unix::signal(SignalKind::terminate())
        .map_err(|error| Failure::Other(format!("signal handler: {error}")))?;
    let mut health = tokio::time::interval(HEALTH_INTERVAL);
    health.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut retry = Backoff::default();

    // Only a rejected login or a quit request ends this loop; a lost network
    // is waited out, so the applet never has to notice.
    let outcome = loop {
        let spirc = Spirc::new(
            connect_config.clone(),
            session.clone(),
            credentials.clone(),
            Arc::clone(&player),
            Arc::clone(&mixer),
        );
        let Some(attempt) = connect(spirc, &mut terminate, &mut health).await else {
            break Ok(());
        };

        match attempt {
            Ok(Ok((spirc, task))) => {
                let connected_at = Instant::now();
                let task = tokio::spawn(task);
                if serve(&spirc, task, &session, &mut terminate, &mut health).await {
                    break Ok(());
                }
                if connected_at.elapsed() >= STABLE_AFTER {
                    retry.reset();
                }
            }
            Ok(Err(error)) if error.to_string().to_lowercase().contains("credentials") => {
                break Err(Failure::NeedsLogin);
            }
            Ok(Err(error)) => log::warn!("could not reach Spotify: {error}"),
            Err(_) => log::warn!(
                "connecting to Spotify took over {}s",
                CONNECT_TIMEOUT.as_secs()
            ),
        }

        retire(&session).await;
        session = Session::new(session_config.clone(), Some(cache.clone()));
        player.set_session(session.clone());
        let delay = retry.next_delay();
        log::info!("reconnecting in {}s", delay.as_secs());
        if !pause(delay, &mut terminate, &mut health).await {
            break Ok(());
        }
    };

    scope.abort();
    let _ = fs::remove_file(paths::scope_socket());
    let _ = fs::remove_file(paths::health_file());
    outcome
}

/// Bounded by [`CONNECT_TIMEOUT`], heartbeat included. `None` when the
/// receiver was asked to quit meanwhile.
async fn connect<T>(
    attempt: impl Future<Output = Result<T, librespot_core::Error>>,
    terminate: &mut Signal,
    health: &mut Interval,
) -> Option<Result<Result<T, librespot_core::Error>, tokio::time::error::Elapsed>> {
    report(Health::Connecting);
    let attempt = tokio::time::timeout(CONNECT_TIMEOUT, attempt);
    tokio::pin!(attempt);
    loop {
        tokio::select! {
            () = quit(terminate) => return None,
            result = &mut attempt => return Some(result),
            _ = health.tick() => report(Health::Connecting),
        }
    }
}

/// Plays until the session drops (false) or the receiver is asked to quit
/// (true). The Connect task ends neither when the session goes invalid nor
/// when the dealer gives up, so both are checked on every heartbeat.
async fn serve(
    spirc: &Spirc,
    mut task: JoinHandle<()>,
    session: &Session,
    terminate: &mut Signal,
    health: &mut Interval,
) -> bool {
    report(Health::Connected);
    log::info!("receiver ready");
    let mut connections = session.dealer().add_listen_for(CONNECTION_ID_URI).ok();
    let mut reconnected = unix_seconds();
    loop {
        tokio::select! {
            () = quit(terminate) => {
                let _ = spirc.shutdown();
                let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
                return true;
            }
            result = &mut task => {
                match result {
                    Ok(()) => log::warn!("session ended"),
                    Err(error) => log::warn!("session task failed: {error}"),
                }
                return false;
            }
            message = next_message(&mut connections) => {
                if message {
                    reconnected = unix_seconds();
                } else {
                    connections = None;
                }
            }
            _ = health.tick() => {
                if session.is_invalid() {
                    log::warn!("session lost its connection to Spotify");
                    task.abort();
                    return false;
                }
                let trouble = DEALER_TROUBLE.load(Ordering::Relaxed);
                if trouble <= reconnected {
                    report(Health::Connected);
                } else if unix_seconds().saturating_sub(trouble) < DEALER_GRACE.as_secs() {
                    report(Health::Connecting);
                } else {
                    log::warn!("Spotify's command channel did not come back; reconnecting");
                    task.abort();
                    return false;
                }
            }
        }
    }
}

/// Waits for the next message of `subscription`: true when one arrived,
/// false once it closed. Without a subscription it never finishes.
async fn next_message(subscription: &mut Option<Subscription>) -> bool {
    match subscription {
        Some(stream) => std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx))
            .await
            .is_some(),
        None => std::future::pending().await,
    }
}

async fn quit(terminate: &mut Signal) {
    tokio::select! {
        _ = terminate.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

/// Waits out a retry delay while keeping the heartbeat fresh. False when the
/// receiver was asked to quit meanwhile.
async fn pause(delay: Duration, terminate: &mut Signal, health: &mut Interval) -> bool {
    let wake = tokio::time::sleep(delay);
    tokio::pin!(wake);
    loop {
        tokio::select! {
            () = quit(terminate) => return false,
            () = &mut wake => return true,
            _ = health.tick() => report(Health::Connecting),
        }
    }
}

/// Also closes the dealer's websocket, which outlives an invalidated session.
async fn retire(session: &Session) {
    if !session.is_invalid() {
        session.shutdown();
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), session.dealer().close()).await;
}

#[derive(Clone, Copy)]
enum Health {
    Connecting,
    Connected,
}

impl Health {
    fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
        }
    }
}

/// Written aside and renamed so a reader never sees half a line.
fn report(health: Health) {
    let path = paths::health_file();
    let mut staging = path.as_os_str().to_owned();
    staging.push(".tmp");
    let written = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&staging)
        .and_then(|mut file| writeln!(file, "{} {}", health.as_str(), unix_seconds()))
        .and_then(|()| fs::rename(&staging, &path));
    if let Err(error) = written {
        log::debug!("cannot write {}: {error}", path.display());
    }
}

struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self { next: RETRY_FIRST }
    }
}

impl Backoff {
    fn reset(&mut self) {
        self.next = RETRY_FIRST;
    }

    fn next_delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = (delay * 2).min(RETRY_MAX);
        delay
    }
}

/// librespot's own software volume.
fn mixer() -> Result<Arc<dyn Mixer>, Failure> {
    mixer::find(Some("softvol"))
        .ok_or_else(|| Failure::Other("the software mixer is not compiled in".into()))?(
        MixerConfig::default(),
    )
    .map_err(other("mixer"))
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
    fn retries_back_off_up_to_a_cap() {
        let mut retry = Backoff::default();
        let delays: Vec<u64> = (0..8).map(|_| retry.next_delay().as_secs()).collect();
        assert_eq!(delays, [1, 2, 4, 8, 16, 32, 60, 60]);
        retry.reset();
        assert_eq!(retry.next_delay(), RETRY_FIRST);
    }

    #[test]
    fn only_connection_failures_count_as_dealer_loss() {
        assert!(is_dealer_loss("Websocket peer does not respond."));
        assert!(is_dealer_loss(
            "Error while connecting: failed to lookup address information"
        ));
        assert!(!is_dealer_loss("No handler for message_ident: hm://x"));
        assert!(!is_dealer_loss("Message couldn't be parsed: eof"));
    }

    #[test]
    fn local_time_is_formatted() {
        let stamp = local_time();
        assert_eq!(stamp.len(), 19, "{stamp}");
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[10..11], " ");
    }
}
