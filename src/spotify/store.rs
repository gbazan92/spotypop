use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Error, User};

const AUTH_FILE: &str = "auth.json";
const AUTH_MAX_BYTES: u64 = 64 * 1024;

/// The Web API login. Only the refresh token is a real secret; the Client ID is not.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAuth {
    pub client_id: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: String,
    /// Unix seconds.
    #[serde(default)]
    pub expires_at: u64,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub authorized_at: u64,
    #[serde(default)]
    pub needs_reauth: bool,
    #[serde(default)]
    pub user: Option<User>,
}

impl fmt::Debug for StoredAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredAuth")
            .field("client_id", &self.client_id)
            .field("refresh_token", &mask(&self.refresh_token))
            .field("access_token", &mask(&self.access_token))
            .field("expires_at", &self.expires_at)
            .field("needs_reauth", &self.needs_reauth)
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

fn mask(value: &str) -> &'static str {
    if value.is_empty() { "" } else { "<redacted>" }
}

#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// `$XDG_STATE_HOME/<app>`; inside Flatpak that already points into `~/.var/app/<id>`.
    pub fn for_app(app: &str) -> Result<Self, Error> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|home| home.join(".local/state"))
            })
            .ok_or_else(|| Error::Storage("no se encontró el directorio de estado".into()))?;
        Ok(Self::at(base.join(app)))
    }

    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn load(&self) -> Result<Option<StoredAuth>, Error> {
        match fs::symlink_metadata(&self.dir) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(storage(&self.dir, &error)),
            Ok(meta) => check_dir(&self.dir, &meta)?,
        }
        let Some(blob) = read_private(&self.dir.join(AUTH_FILE))? else {
            return Ok(None);
        };
        // A corrupt file reads as signed out rather than locking the user out.
        Ok(serde_json::from_slice(&blob).ok())
    }

    pub fn save(&self, auth: &StoredAuth) -> Result<(), Error> {
        self.ensure_dir()?;
        let blob = serde_json::to_vec(auth).map_err(|error| Error::Storage(error.to_string()))?;
        write_atomic(&self.dir, AUTH_FILE, &blob)
    }

    pub fn clear(&self) -> Result<(), Error> {
        match fs::remove_file(self.dir.join(AUTH_FILE)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(storage(&self.dir, &error))
            }
            _ => Ok(()),
        }
    }

    fn ensure_dir(&self) -> Result<(), Error> {
        match DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
        {
            Ok(()) => {}
            Err(error) => return Err(storage(&self.dir, &error)),
        }
        let meta = fs::symlink_metadata(&self.dir).map_err(|error| storage(&self.dir, &error))?;
        check_dir(&self.dir, &meta)?;
        if meta.mode() & 0o077 != 0 {
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
                .map_err(|error| storage(&self.dir, &error))?;
        }
        Ok(())
    }
}

fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

fn storage(path: &Path, error: &io::Error) -> Error {
    Error::Storage(format!("{}: {error}", path.display()))
}

fn unsafe_path(path: &Path, why: &str) -> Error {
    Error::Storage(format!("{} {why}", path.display()))
}

fn check_dir(path: &Path, meta: &fs::Metadata) -> Result<(), Error> {
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(unsafe_path(path, "no es un directorio real"));
    }
    if meta.uid() != euid() {
        return Err(unsafe_path(path, "pertenece a otro usuario"));
    }
    Ok(())
}

/// Refuses symlinks, FIFOs, other owners, hard links, files others can read
/// and anything over the size cap, instead of silently treating them as empty.
fn read_private(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
            return Err(unsafe_path(path, "es un enlace simbólico"));
        }
        Err(error) => return Err(storage(path, &error)),
    };
    let meta = file.metadata().map_err(|error| storage(path, &error))?;
    if !meta.is_file() {
        return Err(unsafe_path(path, "no es un archivo regular"));
    }
    if meta.uid() != euid() {
        return Err(unsafe_path(path, "pertenece a otro usuario"));
    }
    if meta.nlink() != 1 {
        return Err(unsafe_path(path, "tiene más de un enlace"));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(unsafe_path(
            path,
            "es accesible para otros usuarios (usá chmod 600)",
        ));
    }
    if meta.len() > AUTH_MAX_BYTES {
        return Err(unsafe_path(path, "es demasiado grande"));
    }
    let mut blob = Vec::new();
    file.take(AUTH_MAX_BYTES + 1)
        .read_to_end(&mut blob)
        .map_err(|error| storage(path, &error))?;
    if blob.len() as u64 > AUTH_MAX_BYTES {
        return Err(unsafe_path(path, "es demasiado grande"));
    }
    Ok(Some(blob))
}

/// Writes through a fresh, unpredictable temp name created with `O_EXCL` at
/// 0600, then renames it over `name`, so nothing planted beforehand is written through.
fn write_atomic(dir: &Path, name: &str, data: &[u8]) -> Result<(), Error> {
    let target = dir.join(name);
    if let Ok(meta) = fs::symlink_metadata(&target)
        && (!meta.is_file() || meta.uid() != euid())
    {
        return Err(unsafe_path(
            &target,
            "no es un archivo propio; no se reemplaza",
        ));
    }

    let suffix: u128 = rand::random();
    let tmp = dir.join(format!(".{name}.{suffix:032x}.tmp"));

    let result = (|| -> io::Result<()> {
        let mut file: File = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&tmp, &target)
    })();

    if let Err(error) = result {
        let _ = fs::remove_file(&tmp);
        return Err(storage(&target, &error));
    }
    if let Ok(dir) = File::open(dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn sample() -> StoredAuth {
        StoredAuth {
            client_id: "client".into(),
            refresh_token: "refresh-secret".into(),
            access_token: "access-secret".into(),
            expires_at: 42,
            user: Some(User {
                id: "u".into(),
                name: "Gerardo".into(),
            }),
            ..StoredAuth::default()
        }
    }

    #[test]
    fn round_trips_with_private_permissions() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::at(root.path().join("state/app"));
        assert_eq!(store.load().unwrap(), None);

        store.save(&sample()).unwrap();
        assert_eq!(store.load().unwrap(), Some(sample()));

        let dir_mode = fs::metadata(root.path().join("state/app")).unwrap().mode() & 0o777;
        let file_mode = fs::metadata(root.path().join("state/app/auth.json"))
            .unwrap()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);

        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.clear().unwrap();
    }

    #[test]
    fn refuses_a_symlinked_auth_file() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::at(root.path().join("app"));
        store.save(&sample()).unwrap();
        let elsewhere = root.path().join("elsewhere.json");
        fs::rename(root.path().join("app/auth.json"), &elsewhere).unwrap();
        symlink(&elsewhere, root.path().join("app/auth.json")).unwrap();

        assert!(store.load().is_err());
        assert!(store.save(&sample()).is_err());
    }

    #[test]
    fn refuses_a_world_readable_auth_file() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::at(root.path().join("app"));
        store.save(&sample()).unwrap();
        fs::set_permissions(
            root.path().join("app/auth.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();

        assert!(store.load().is_err());
    }

    #[test]
    fn corrupt_file_reads_as_signed_out() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::at(root.path().join("app"));
        store.save(&sample()).unwrap();
        write_atomic(&root.path().join("app"), AUTH_FILE, b"{not json").unwrap();

        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn debug_output_hides_tokens() {
        let rendered = format!("{:?}", sample());
        assert!(!rendered.contains("secret"));
    }
}
