//! The local receiver (`spotypop-player`): finding it, authorizing
//! it once, and keeping one instance running outside the panel's lifetime.
//! Paths mirror `player/src/paths.rs`.

use std::env;
use std::ffi::CStr;
use std::fs::{self, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::fl;

pub const BINARY: &str = "spotypop-player";
const APP_DIR: &str = "spotypop";
/// Exit code of `run` and `login` when the computer has no playback login.
const NEEDS_LOGIN: i32 = 3;
/// The receiver rewrites its heartbeat every 5 s, also while it waits to
/// retry, and a connection attempt is capped at 20 s.
const HEARTBEAT_STALE: Duration = Duration::from_secs(30);
const STOP_GRACE: Duration = Duration::from_secs(3);

fn xdg(var: &str, fallback: &str) -> PathBuf {
    env::var_os(var)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback)))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join(APP_DIR)
}

/// A small file of the applet's own under the state directory.
pub fn state_file(name: &str) -> PathBuf {
    state_dir().join(name)
}

fn credentials_dir() -> PathBuf {
    state_dir().join("player")
}

fn runtime_dir() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        // SAFETY: getuid cannot fail.
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })))
        .join(APP_DIR)
}

/// A small file under the private runtime directory shared by every instance.
pub fn runtime_file(name: &str) -> PathBuf {
    runtime_dir().join(name)
}

fn pid_file() -> PathBuf {
    runtime_dir().join("player.pid")
}

fn health_file() -> PathBuf {
    runtime_dir().join("player.health")
}

/// Where the receiver streams what is sounding; see `src/feed.rs`.
pub fn scope_socket() -> PathBuf {
    runtime_dir().join("scope.sock")
}

/// Installed next to the applet, both by `just install` and in the Flatpak.
pub fn binary() -> Option<PathBuf> {
    let path = env::current_exe().ok()?.parent()?.join(BINARY);
    path.is_file().then_some(path)
}

/// How the receiver shows up in Spotify's device lists.
pub fn device_name() -> String {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer outlives the call and its length is passed along.
    let host = if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } == 0 {
        CStr::from_bytes_until_nul(&buffer)
            .ok()
            .and_then(|name| name.to_str().ok())
            .map(str::to_owned)
    } else {
        None
    };
    match host.filter(|host| !host.is_empty()) {
        Some(host) => format!("{host} (COSMIC)"),
        None => "COSMIC".to_owned(),
    }
}

pub fn authorized() -> bool {
    fs::symlink_metadata(credentials_dir().join("credentials.json"))
        .is_ok_and(|meta| meta.is_file() && meta.uid() == unsafe { libc::getuid() })
}

fn log_file(name: &str) -> Option<fs::File> {
    let dir = state_dir();
    fs::create_dir_all(&dir).ok()?;
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(name))
        .ok()
}

/// Starts the receiver in its own session so a panel restart does not take
/// the music with it. A second instance notices the first and exits.
///
/// The receiver opens its log itself once it holds the instance lock; opening
/// it here would truncate the log of the receiver that is already playing.
pub fn start() -> Result<(), String> {
    let binary = binary().ok_or_else(|| fl!("player-not-found", binary = BINARY))?;
    let mut command = Command::new(binary);
    command
        .arg("run")
        .arg("--name")
        .arg(device_name())
        .arg("--log")
        .arg(state_file("player.log"))
        .env_remove("XDG_ACTIVATION_TOKEN")
        .env_remove("DESKTOP_STARTUP_ID")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|error| fl!("player-start-failed", error = error.to_string()))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// The receiver's pid, if the pid file still points at a live one. A stale
/// file could name an unrelated process by now, hence the executable check.
fn running_pid() -> Option<libc::pid_t> {
    let pid = fs::read_to_string(pid_file())
        .ok()?
        .trim()
        .parse::<libc::pid_t>()
        .ok()
        .filter(|pid| *pid > 1)?;
    // After a reinstall the kernel reports the old binary as "... (deleted)".
    let ours = fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| {
        exe.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(BINARY))
    });
    ours.then_some(pid)
}

/// What the receiver is doing, as far as its heartbeat tells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Receiver {
    Stopped,
    /// Starting up, or waiting for Spotify or the network to come back.
    Connecting,
    Connected,
    /// Alive but no longer writing its heartbeat.
    Stuck,
}

fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// `heartbeat` is the health file's content; `claimed` is when the receiver
/// wrote its pid file, which covers the moments before its first heartbeat.
fn classify(heartbeat: Option<&str>, claimed: Option<u64>, now: u64) -> Receiver {
    let fresh = |at: u64| now.saturating_sub(at) <= HEARTBEAT_STALE.as_secs();
    let beat = heartbeat.and_then(|line| {
        let (state, at) = line.trim().split_once(' ')?;
        Some((state, at.parse::<u64>().ok()?))
    });
    match beat {
        Some(("connected", at)) if fresh(at) => Receiver::Connected,
        Some((_, at)) if fresh(at) => Receiver::Connecting,
        _ if claimed.is_some_and(fresh) => Receiver::Connecting,
        _ => Receiver::Stuck,
    }
}

/// Whether some receiver holds the instance lock. Unlike the pid, this works
/// across Flatpak sandboxes, whose pid namespaces hide each other's processes.
/// The probe holds the lock for an instant; a receiver starting right then
/// takes it for a duplicate and exits, and the next poll starts it again.
fn lock_held() -> bool {
    let Ok(file) = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(pid_file())
    else {
        return false;
    };
    // SAFETY: flock only reads the descriptor, which `file` keeps open;
    // dropping `file` releases a lock the probe itself took.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) != 0 }
}

pub fn status() -> Receiver {
    if !lock_held() {
        return Receiver::Stopped;
    }
    let heartbeat = fs::read_to_string(health_file()).ok();
    let claimed = fs::metadata(pid_file())
        .and_then(|meta| meta.modified())
        .ok()
        .map(unix_seconds);
    classify(
        heartbeat.as_deref(),
        claimed,
        unix_seconds(SystemTime::now()),
    )
}

fn alive(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

pub fn stop() {
    if let Some(pid) = running_pid() {
        // SAFETY: plain signal delivery to a process we verified.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}

/// Stops a receiver that stopped answering, killing it if SIGTERM is not
/// enough, and starts a fresh one. Runs on its own thread because the wait
/// would freeze the panel.
pub fn restart() {
    std::thread::spawn(|| {
        if running_pid().is_none() && lock_held() {
            eprintln!("the stuck receiver was started by another applet instance");
            return;
        }
        if let Some(pid) = running_pid() {
            // SAFETY: plain signal delivery to a process we verified.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            let step = Duration::from_millis(100);
            let mut waited = Duration::ZERO;
            while alive(pid) && waited < STOP_GRACE {
                std::thread::sleep(step);
                waited += step;
            }
            if alive(pid) {
                eprintln!("receiver ignored SIGTERM; killing it");
                // SAFETY: the pid verified above, a few seconds ago.
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
                std::thread::sleep(step);
            }
        }
        if let Err(error) = start() {
            eprintln!("{error}");
        }
    });
}

/// Stops the receiver and deletes its login.
pub fn forget() {
    stop();
    let _ = fs::remove_dir_all(credentials_dir());
}

/// Runs the one-time browser authorization. The activation token lets the
/// browser come to the front. Dropping the future kills the child.
///
/// Output goes to a file, never a pipe: the browser it launches inherits the
/// descriptors and would hold a pipe open long after the login is done.
pub async fn login(token: Option<String>) -> Result<(), String> {
    let binary = binary().ok_or_else(|| fl!("player-not-found", binary = BINARY))?;
    let log_path = state_dir().join("player-login.log");
    let stderr = log_file("player-login.log").map_or_else(Stdio::null, Stdio::from);
    let mut command = tokio::process::Command::new(binary);
    command
        .arg("login")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .kill_on_drop(true);
    match token {
        Some(token) => {
            command
                .env("XDG_ACTIVATION_TOKEN", &token)
                .env("DESKTOP_STARTUP_ID", &token);
        }
        None => {
            command
                .env_remove("XDG_ACTIVATION_TOKEN")
                .env_remove("DESKTOP_STARTUP_ID");
        }
    }
    let status = command
        .status()
        .await
        .map_err(|error| fl!("player-start-failed", error = error.to_string()))?;
    if status.success() {
        return Ok(());
    }
    if status.code() == Some(NEEDS_LOGIN) {
        return Err(fl!("player-not-authorized"));
    }
    let detail = fs::read_to_string(log_path).unwrap_or_default();
    let last = detail.lines().rev().find(|line| !line.trim().is_empty());
    Err(match last {
        Some(line) => fl!(
            "player-auth-failed",
            reason = line.chars().take(160).collect::<String>()
        ),
        None => fl!("player-auth-incomplete"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_name_mentions_cosmic() {
        assert!(device_name().contains("COSMIC"));
    }

    #[test]
    fn fresh_heartbeats_report_their_state() {
        assert_eq!(
            classify(Some("connected 1000\n"), Some(10), 1020),
            Receiver::Connected
        );
        assert_eq!(
            classify(Some("connecting 1000\n"), Some(10), 1020),
            Receiver::Connecting
        );
    }

    #[test]
    fn a_silent_receiver_is_stuck_once_past_its_startup() {
        assert_eq!(
            classify(Some("connected 1000"), Some(10), 1031),
            Receiver::Stuck
        );
        assert_eq!(classify(None, Some(10), 1000), Receiver::Stuck);
        assert_eq!(classify(Some("garbage"), None, 1000), Receiver::Stuck);
    }

    #[test]
    fn a_receiver_that_just_started_gets_time_to_report() {
        assert_eq!(classify(None, Some(995), 1000), Receiver::Connecting);
        assert_eq!(
            classify(Some("connected 1"), Some(995), 1000),
            Receiver::Connecting
        );
    }
}
