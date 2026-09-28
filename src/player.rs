//! The local receiver (`cosmic-ext-spotify-player`): finding it, authorizing
//! it once, and keeping one instance running outside the panel's lifetime.
//! Paths mirror `player/src/paths.rs`.

use std::env;
use std::ffi::CStr;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const BINARY: &str = "cosmic-ext-spotify-player";
const APP_DIR: &str = "cosmic-ext-applet-spotify";
/// Exit code of `run` and `login` when the computer has no playback login.
const NEEDS_LOGIN: i32 = 3;

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

fn credentials_dir() -> PathBuf {
    state_dir().join("player")
}

fn pid_file() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        // SAFETY: getuid cannot fail.
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })))
        .join(APP_DIR)
        .join("player.pid")
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
pub fn start() -> Result<(), String> {
    let binary = binary().ok_or_else(|| format!("no se encontró {BINARY}"))?;
    let stderr = log_file("player.log").map_or_else(Stdio::null, Stdio::from);
    let mut command = Command::new(binary);
    command
        .arg("run")
        .arg("--name")
        .arg(device_name())
        .env_remove("XDG_ACTIVATION_TOKEN")
        .env_remove("DESKTOP_STARTUP_ID")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("no se pudo iniciar el reproductor: {error}"))?;
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

pub fn running() -> bool {
    running_pid().is_some()
}

pub fn stop() {
    if let Some(pid) = running_pid() {
        // SAFETY: plain signal delivery to a process we verified.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
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
    let binary = binary().ok_or_else(|| format!("no se encontró {BINARY}"))?;
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
        .map_err(|error| format!("no se pudo iniciar el reproductor: {error}"))?;
    if status.success() {
        return Ok(());
    }
    if status.code() == Some(NEEDS_LOGIN) {
        return Err("Spotify no autorizó esta compu.".into());
    }
    let detail = fs::read_to_string(log_path).unwrap_or_default();
    let last = detail.lines().rev().find(|line| !line.trim().is_empty());
    Err(match last {
        Some(line) => format!(
            "La autorización falló: {}",
            line.chars().take(160).collect::<String>()
        ),
        None => "La autorización no se completó.".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_name_mentions_cosmic() {
        assert!(device_name().contains("COSMIC"));
    }
}
