use std::path::Path;
use std::process::{Command, Stdio};

/// Inside Flatpak `/usr/bin/xdg-open` is the portal wrapper, which forwards the token too.
const XDG_OPEN: &[&str] = &["/usr/bin/xdg-open", "/bin/xdg-open", "/app/bin/xdg-open"];

/// Opens `url` in the default browser. With an xdg-activation token the
/// compositor raises and focuses the browser; without one Wayland's focus
/// stealing prevention leaves it behind the current window.
pub fn open(url: &str, activation_token: Option<&str>) -> Result<(), String> {
    let Some(program) = XDG_OPEN.iter().find(|path| Path::new(path).is_file()) else {
        return open::that_detached(url).map_err(|error| error.to_string());
    };

    let mut command = Command::new(program);
    command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match activation_token {
        Some(token) => {
            command
                .env("XDG_ACTIVATION_TOKEN", token)
                .env("DESKTOP_STARTUP_ID", token);
        }
        // A token inherited from the panel is single-use and already spent.
        None => {
            command
                .env_remove("XDG_ACTIVATION_TOKEN")
                .env_remove("DESKTOP_STARTUP_ID");
        }
    }

    let mut child = command.spawn().map_err(|error| error.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
