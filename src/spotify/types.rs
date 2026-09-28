use serde::{Deserialize, Serialize};

use super::Error;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    Context,
    Track,
}

impl Repeat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Context => "context",
            Self::Track => "track",
        }
    }

    /// The order Spotify's own clients cycle through.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Context,
            Self::Context => Self::Track,
            Self::Track => Self::Off,
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "context" => Self::Context,
            "track" => Self::Track,
            _ => Self::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Track,
    Episode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub kind: ItemKind,
    pub id: String,
    pub uri: String,
    pub name: String,
    /// Artists for a track, the show for an episode.
    pub subtitle: String,
    pub album: String,
    /// Album or show the item belongs to, so play can carry on after it.
    #[serde(default)]
    pub parent_uri: Option<String>,
    pub duration_ms: u64,
    pub art_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: Option<String>,
    pub name: String,
    pub kind: String,
    pub is_active: bool,
    pub is_restricted: bool,
    pub volume: Option<u8>,
    pub supports_volume: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerState {
    pub is_playing: bool,
    pub progress_ms: u64,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub device: Option<Device>,
    pub item: Option<Item>,
}

// Raw Web API shapes. Every field is optional because Spotify omits or nulls
// them freely (local files, podcasts, restricted devices).

#[derive(Deserialize)]
pub(super) struct RawPlayer {
    #[serde(default)]
    is_playing: bool,
    progress_ms: Option<u64>,
    #[serde(default)]
    shuffle_state: bool,
    repeat_state: Option<String>,
    device: Option<RawDevice>,
    item: Option<RawItem>,
}

#[derive(Deserialize)]
pub(super) struct RawDevice {
    id: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    is_restricted: bool,
    volume_percent: Option<u8>,
    supports_volume: Option<bool>,
}

#[derive(Deserialize)]
pub(super) struct RawDevices {
    #[serde(default)]
    pub devices: Vec<RawDevice>,
}

#[derive(Deserialize)]
struct RawItem {
    #[serde(default, rename = "type")]
    kind: String,
    id: Option<String>,
    #[serde(default)]
    uri: String,
    #[serde(default)]
    name: String,
    duration_ms: Option<u64>,
    #[serde(default)]
    artists: Vec<RawNamed>,
    album: Option<RawAlbum>,
    show: Option<RawShow>,
    #[serde(default)]
    images: Vec<RawImage>,
}

#[derive(Deserialize)]
struct RawNamed {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct RawAlbum {
    #[serde(default)]
    uri: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    images: Vec<RawImage>,
}

#[derive(Deserialize)]
struct RawShow {
    #[serde(default)]
    uri: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    images: Vec<RawImage>,
}

#[derive(Deserialize)]
struct RawImage {
    url: String,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Deserialize)]
pub(super) struct RawMe {
    #[serde(default)]
    id: String,
    display_name: Option<String>,
}

impl From<RawMe> for User {
    fn from(raw: RawMe) -> Self {
        let name = raw
            .display_name
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| raw.id.clone());
        Self { id: raw.id, name }
    }
}

/// Cover size requested for the popup; the panel scales the same image down.
const ART_SIZE: u32 = 300;

impl From<RawPlayer> for PlayerState {
    fn from(raw: RawPlayer) -> Self {
        Self {
            is_playing: raw.is_playing,
            progress_ms: raw.progress_ms.unwrap_or_default(),
            shuffle: raw.shuffle_state,
            repeat: Repeat::parse(raw.repeat_state.as_deref().unwrap_or("off")),
            device: raw.device.map(Device::from),
            item: raw.item.and_then(Item::from_raw),
        }
    }
}

impl From<RawDevice> for Device {
    fn from(raw: RawDevice) -> Self {
        Self {
            id: raw.id.filter(|id| !id.is_empty()),
            name: raw.name,
            kind: raw.kind,
            is_active: raw.is_active,
            is_restricted: raw.is_restricted,
            volume: raw.volume_percent.map(|volume| volume.min(100)),
            supports_volume: raw.supports_volume.unwrap_or(true),
        }
    }
}

impl Item {
    fn from_raw(raw: RawItem) -> Option<Self> {
        let (kind, subtitle, album, parent_uri, images) = match raw.kind.as_str() {
            "track" => {
                let artists = raw
                    .artists
                    .iter()
                    .map(|artist| artist.name.as_str())
                    .filter(|name| !name.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ");
                let (album, parent, images) = raw
                    .album
                    .map(|album| (album.name, album.uri, album.images))
                    .unwrap_or_default();
                (ItemKind::Track, artists, album, parent, images)
            }
            "episode" => {
                let (show, parent, show_images) = raw
                    .show
                    .map(|show| (show.name, show.uri, show.images))
                    .unwrap_or_default();
                let images = if raw.images.is_empty() {
                    show_images
                } else {
                    raw.images
                };
                (ItemKind::Episode, show, String::new(), parent, images)
            }
            _ => return None,
        };

        Some(Self {
            kind,
            id: raw.id.unwrap_or_default(),
            uri: raw.uri,
            name: raw.name,
            subtitle,
            album,
            parent_uri: Some(parent_uri).filter(|uri| !uri.is_empty()),
            duration_ms: raw.duration_ms.unwrap_or_default(),
            art_url: pick_image(&images, ART_SIZE),
        })
    }
}

fn pick_image(images: &[RawImage], prefer: u32) -> Option<String> {
    pick_url(
        images.iter().map(|image| {
            (
                image.url.as_str(),
                image.width.or(image.height).unwrap_or(0),
            )
        }),
        prefer,
    )
}

/// The smallest `(url, size)` at least `prefer` px wide, else the largest one.
pub(super) fn pick_url<'a>(
    images: impl IntoIterator<Item = (&'a str, u32)>,
    prefer: u32,
) -> Option<String> {
    let mut sorted: Vec<(&str, u32)> = images
        .into_iter()
        .filter(|(url, _)| !url.is_empty())
        .collect();
    sorted.sort_by_key(|&(_, size)| size);
    sorted
        .iter()
        .find(|&&(_, size)| size >= prefer)
        .or(sorted.last())
        .map(|&(url, _)| url.to_owned())
}

/// Spotify IDs are base62; anything else must not reach a URL path.
pub fn check_id(value: &str) -> Result<&str, Error> {
    if !value.is_empty() && value.len() <= 64 && value.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Ok(value)
    } else {
        Err(Error::BadArgument(format!(
            "invalid Spotify ID: {value:.40}"
        )))
    }
}

pub fn check_uri(value: &str) -> Result<&str, Error> {
    let valid = value.len() <= 160
        && value.strip_prefix("spotify:").is_some_and(|rest| {
            let mut parts = rest.splitn(2, ':');
            let kind = parts.next().unwrap_or_default();
            let tail = parts.next().unwrap_or_default();
            !kind.is_empty()
                && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                && !tail.is_empty()
                && tail
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'-'))
        });
    if valid {
        Ok(value)
    } else {
        Err(Error::BadArgument(format!(
            "invalid Spotify URI: {value:.60}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: &str = r#"{
        "device": {"id": "abc", "is_active": true, "is_restricted": false, "name": "Omarchy Spotify",
                   "type": "Computer", "volume_percent": 64, "supports_volume": true},
        "shuffle_state": true, "repeat_state": "context", "progress_ms": 12345, "is_playing": true,
        "item": {
            "type": "track", "id": "4uLU6hMCjMI75M1A2tKUQC", "uri": "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
            "name": "Never Gonna Give You Up", "duration_ms": 213573,
            "artists": [{"name": "Rick Astley"}, {"name": ""}],
            "album": {"name": "Whenever You Need Somebody", "images": [
                {"url": "https://i.scdn.co/image/640", "width": 640, "height": 640},
                {"url": "https://i.scdn.co/image/64", "width": 64, "height": 64},
                {"url": "https://i.scdn.co/image/300", "width": 300, "height": 300}
            ]}
        }
    }"#;

    #[test]
    fn parses_a_track_player_state() {
        let state = PlayerState::from(serde_json::from_str::<RawPlayer>(PLAYER).unwrap());
        assert!(state.is_playing);
        assert!(state.shuffle);
        assert_eq!(state.repeat, Repeat::Context);
        assert_eq!(state.progress_ms, 12345);
        let device = state.device.unwrap();
        assert_eq!(device.volume, Some(64));
        assert_eq!(device.id.as_deref(), Some("abc"));
        let item = state.item.unwrap();
        assert_eq!(item.kind, ItemKind::Track);
        assert_eq!(item.subtitle, "Rick Astley");
        assert_eq!(item.album, "Whenever You Need Somebody");
        assert_eq!(item.art_url.as_deref(), Some("https://i.scdn.co/image/300"));
    }

    #[test]
    fn episodes_fall_back_to_show_art() {
        let raw = r#"{"is_playing": false, "item": {"type": "episode", "id": "e1", "uri": "spotify:episode:e1",
            "name": "Ep", "images": [], "show": {"name": "Pod", "images": [{"url": "https://i.scdn.co/s", "width": 64}]}}}"#;
        let item = PlayerState::from(serde_json::from_str::<RawPlayer>(raw).unwrap())
            .item
            .unwrap();
        assert_eq!(item.kind, ItemKind::Episode);
        assert_eq!(item.subtitle, "Pod");
        assert_eq!(item.art_url.as_deref(), Some("https://i.scdn.co/s"));
    }

    #[test]
    fn tolerates_nulls_and_unknown_items() {
        let raw = r#"{"device": null, "item": {"type": "ad"}, "repeat_state": null}"#;
        let state = PlayerState::from(serde_json::from_str::<RawPlayer>(raw).unwrap());
        assert!(state.device.is_none());
        assert!(state.item.is_none());
        assert_eq!(state.repeat, Repeat::Off);
    }

    #[test]
    fn repeat_cycles_like_spotify() {
        assert_eq!(Repeat::Off.next(), Repeat::Context);
        assert_eq!(Repeat::Context.next(), Repeat::Track);
        assert_eq!(Repeat::Track.next(), Repeat::Off);
    }

    #[test]
    fn validates_ids_and_uris() {
        assert!(check_id("4uLU6hMCjMI75M1A2tKUQC").is_ok());
        assert!(check_id("../me").is_err());
        assert!(check_id("").is_err());
        assert!(check_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC").is_ok());
        assert!(check_uri("spotify:user:abc:collection").is_ok());
        assert!(check_uri("spotify:track:a b").is_err());
        assert!(check_uri("https://open.spotify.com/track/x").is_err());
        assert!(check_uri("spotify::x").is_err());
    }
}
