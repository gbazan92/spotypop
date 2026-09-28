//! Where the receiver keeps its files. The applet mirrors these in `src/player.rs`.

use std::env;
use std::path::PathBuf;

const APP_DIR: &str = "spotypop";

fn xdg(var: &str, fallback: &str) -> PathBuf {
    env::var_os(var)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback)))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// The reusable playback login; survives cache cleanups.
pub fn credentials_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state")
        .join(APP_DIR)
        .join("player")
}

pub fn audio_cache_dir() -> PathBuf {
    xdg("XDG_CACHE_HOME", ".cache").join(APP_DIR).join("audio")
}

fn runtime_dir() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        // SAFETY: getuid cannot fail.
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })))
        .join(APP_DIR)
}

pub fn pid_file() -> PathBuf {
    runtime_dir().join("player.pid")
}

/// Lives next to the pid file, whose directory is private to the user.
pub fn scope_socket() -> PathBuf {
    runtime_dir().join("scope.sock")
}
