//! One applet per panel slot. The panel can lose track of an instance it
//! launched and start another; the lost one would keep polling Spotify, and
//! holding the receiver, with nothing on screen. Each instance claims its slot
//! when it starts and quits once a newer one has taken it.
//!
//! Instances in separate Flatpak sandboxes cannot see or signal each other's
//! processes, so the handover goes through a file in the shared runtime dir.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;

use crate::player;

pub struct Slot {
    path: PathBuf,
    token: String,
}

/// The same panel on another output, or the dock, is a different slot.
fn slot_key(name: &str, output: &str) -> String {
    format!("{name}-{output}")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl Slot {
    /// `None` when not launched by the panel, or when the claim cannot be
    /// written; the applet then simply runs unsupervised.
    pub fn claim() -> Option<Self> {
        let name = env::var("COSMIC_PANEL_NAME").ok()?;
        let output = env::var("COSMIC_PANEL_OUTPUT").unwrap_or_default();
        let path = player::runtime_file(&format!("applet-{}.owner", slot_key(&name, &output)));
        let token = format!("{:032x}", rand::random::<u128>());

        let dir = path.parent()?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .ok()?;
        let mut staging = path.as_os_str().to_owned();
        staging.push(".tmp");
        let written = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&staging)
            .and_then(|mut file| writeln!(file, "{token}"))
            .and_then(|()| fs::rename(&staging, &path));
        if let Err(error) = written {
            eprintln!("cannot claim the panel slot: {error}");
            return None;
        }
        Some(Self { path, token })
    }

    /// A missing file is not a takeover: only a different owner is.
    pub fn taken_over(&self) -> bool {
        fs::read_to_string(&self.path).is_ok_and(|owner| owner.trim() != self.token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_keys_are_safe_file_names() {
        assert_eq!(slot_key("Panel", "HDMI-A-1"), "Panel-HDMI-A-1");
        assert_eq!(slot_key("My Dock", "../x"), "My_Dock-___x");
    }
}
