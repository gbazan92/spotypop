//! Browsable things: search results, library collections and their contents.
//! Parsed from loose JSON because Spotify nulls or omits fields freely and the
//! same object shows up in many envelopes.

use serde_json::Value;

use super::types::pick_url;

/// List thumbnails; the smallest Spotify image is 64 px.
const THUMB_SIZE: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    Track,
    Episode,
    Chapter,
    Album,
    Artist,
    Playlist,
    Show,
    Audiobook,
}

impl EntryKind {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "track" => Self::Track,
            "episode" => Self::Episode,
            "chapter" => Self::Chapter,
            "album" => Self::Album,
            "artist" => Self::Artist,
            "playlist" => Self::Playlist,
            "show" => Self::Show,
            "audiobook" => Self::Audiobook,
            _ => return None,
        })
    }

    pub fn search_type(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Episode => "episode",
            Self::Chapter => "chapter",
            Self::Album => "album",
            Self::Artist => "artist",
            Self::Playlist => "playlist",
            Self::Show => "show",
            Self::Audiobook => "audiobook",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            Self::Track => "tracks",
            Self::Episode => "episodes",
            Self::Chapter => "chapters",
            Self::Album => "albums",
            Self::Artist => "artists",
            Self::Playlist => "playlists",
            Self::Show => "shows",
            Self::Audiobook => "audiobooks",
        }
    }

    /// Single playable items, as opposed to contexts that hold them.
    pub fn is_item(self) -> bool {
        matches!(self, Self::Track | Self::Episode | Self::Chapter)
    }

    /// Contexts whose contents can be listed.
    pub fn opens(self) -> bool {
        matches!(
            self,
            Self::Album | Self::Playlist | Self::Show | Self::Audiobook
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub id: String,
    pub uri: String,
    pub name: String,
    /// Second line: artists, owner, publisher…
    pub detail: String,
    /// Album, show or audiobook a single item belongs to.
    pub parent_uri: Option<String>,
    pub duration_ms: u64,
    pub art_url: Option<String>,
    pub playable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchGroup {
    pub kind: EntryKind,
    pub entries: Vec<Entry>,
}

pub const SEARCH_KINDS: [EntryKind; 7] = [
    EntryKind::Track,
    EntryKind::Artist,
    EntryKind::Album,
    EntryKind::Playlist,
    EntryKind::Show,
    EntryKind::Episode,
    EntryKind::Audiobook,
];

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn obj<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|inner| inner.is_object())
}

fn names(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|entry| str_of(entry, "name"))
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn images(value: &Value) -> Option<String> {
    let list = value.get("images").and_then(Value::as_array)?;
    let size = |image: &Value| {
        let side = image
            .get("width")
            .and_then(Value::as_u64)
            .or_else(|| image.get("height").and_then(Value::as_u64))
            .unwrap_or_default();
        u32::try_from(side).unwrap_or(u32::MAX)
    };
    pick_url(
        list.iter().map(|image| (str_of(image, "url"), size(image))),
        THUMB_SIZE,
    )
}

fn count(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

fn join(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("  ·  ")
}

/// One Web API object of any browsable type. `parent` stands in for the album,
/// show or audiobook when the object comes from inside one and omits it.
pub fn entry(value: &Value, parent: Option<&Value>) -> Option<Entry> {
    let kind = EntryKind::parse(str_of(value, "type"))?;
    let uri = str_of(value, "uri");
    if uri.is_empty() {
        return None;
    }

    let mut parent_uri = None;
    let mut duration_ms = 0;
    let mut playable = true;
    let (detail, art_url) = match kind {
        EntryKind::Track => {
            let album = obj(value, "album").or(parent);
            parent_uri = album.map(|album| str_of(album, "uri").to_owned());
            duration_ms = count(value, "duration_ms");
            playable = value.get("is_playable").and_then(Value::as_bool) != Some(false);
            (
                names(value, "artists"),
                album.and_then(images).or_else(|| images(value)),
            )
        }
        EntryKind::Episode | EntryKind::Chapter => {
            let key = if kind == EntryKind::Episode {
                "show"
            } else {
                "audiobook"
            };
            let owner = obj(value, key).or(parent);
            parent_uri = owner.map(|owner| str_of(owner, "uri").to_owned());
            duration_ms = count(value, "duration_ms");
            (
                owner
                    .map(|owner| str_of(owner, "name").to_owned())
                    .unwrap_or_default(),
                images(value).or_else(|| owner.and_then(images)),
            )
        }
        EntryKind::Album => {
            let year = str_of(value, "release_date").get(..4).unwrap_or_default();
            (
                join(&["Álbum", &names(value, "artists"), year]),
                images(value),
            )
        }
        EntryKind::Artist => ("Artista".to_owned(), images(value)),
        EntryKind::Playlist => {
            let owner = obj(value, "owner").map_or("", |owner| {
                let name = str_of(owner, "display_name");
                if name.is_empty() {
                    str_of(owner, "id")
                } else {
                    name
                }
            });
            // 2026 renamed the counter from `tracks` to `items`.
            let total = obj(value, "items")
                .or_else(|| obj(value, "tracks"))
                .map_or(0, |counter| count(counter, "total"));
            let total = if total > 0 {
                format!("{total} canciones")
            } else {
                String::new()
            };
            (join(&["Playlist", owner, &total]), images(value))
        }
        EntryKind::Show => (
            join(&["Podcast", str_of(value, "publisher")]),
            images(value),
        ),
        EntryKind::Audiobook => (
            join(&["Audiolibro", &names(value, "authors")]),
            images(value),
        ),
    };

    Some(Entry {
        kind,
        id: str_of(value, "id").to_owned(),
        uri: uri.to_owned(),
        name: str_of(value, "name").to_owned(),
        detail,
        parent_uri: parent_uri.filter(|uri| !uri.is_empty()),
        duration_ms,
        art_url,
        playable,
    })
}

/// Entries from a page's `items`, unwrapping envelopes such as
/// `{"added_at", "track": {…}}` or `{"item": {…}}`.
pub fn entries(page: &Value, envelope: Option<&str>, parent: Option<&Value>) -> Vec<Entry> {
    page.get("items")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|raw| {
                    let inner = match envelope {
                        Some(key) => obj(raw, key)
                            .or_else(|| obj(raw, "item"))
                            .or_else(|| obj(raw, "track"))?,
                        None => raw,
                    };
                    entry(inner, parent)
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn search_groups(payload: &Value) -> Vec<SearchGroup> {
    SEARCH_KINDS
        .iter()
        .filter_map(|&kind| {
            let entries = payload
                .get(kind.plural())
                .map(|bucket| entries(bucket, None, None))
                .unwrap_or_default();
            (!entries.is_empty()).then_some(SearchGroup { kind, entries })
        })
        .collect()
}

/// Recently played tracks, newest first, each track once.
pub fn recent(payload: &Value, limit: usize) -> Vec<Entry> {
    let mut seen = std::collections::HashSet::new();
    entries(payload, Some("track"), None)
        .into_iter()
        .filter(|entry| seen.insert(entry.uri.clone()))
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_tracks_with_album_art_and_parent() {
        let raw = json!({
            "type": "track", "id": "t1", "uri": "spotify:track:t1", "name": "Song",
            "duration_ms": 1000, "artists": [{"name": "A"}, {"name": "B"}],
            "album": {"uri": "spotify:album:a1", "images": [
                {"url": "https://i.scdn.co/640", "width": 640},
                {"url": "https://i.scdn.co/64", "width": 64}
            ]}
        });
        let entry = entry(&raw, None).unwrap();
        assert_eq!(entry.kind, EntryKind::Track);
        assert_eq!(entry.detail, "A, B");
        assert_eq!(entry.parent_uri.as_deref(), Some("spotify:album:a1"));
        assert_eq!(entry.art_url.as_deref(), Some("https://i.scdn.co/64"));
        assert!(entry.playable);
    }

    #[test]
    fn album_tracks_borrow_the_album_as_parent() {
        let album = json!({"uri": "spotify:album:a1", "images": [{"url": "https://i.scdn.co/a", "width": 300}]});
        let page = json!({"items": [{"type": "track", "uri": "spotify:track:t1", "name": "x"}]});
        let list = entries(&page, None, Some(&album));
        assert_eq!(list[0].parent_uri.as_deref(), Some("spotify:album:a1"));
        assert_eq!(list[0].art_url.as_deref(), Some("https://i.scdn.co/a"));
    }

    #[test]
    fn unwraps_playlist_and_saved_envelopes() {
        let page = json!({"items": [
            {"added_at": "x", "item": {"type": "track", "uri": "spotify:track:1", "name": "new"}},
            {"added_at": "x", "track": {"type": "track", "uri": "spotify:track:2", "name": "old"}},
            {"added_at": "x", "track": null},
            null
        ]});
        let list = entries(&page, Some("track"), None);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn playlist_counts_use_the_new_items_field() {
        let raw = json!({"type": "playlist", "uri": "spotify:playlist:p", "name": "Mix",
            "owner": {"id": "me", "display_name": ""}, "items": {"total": 12}});
        assert_eq!(
            entry(&raw, None).unwrap().detail,
            "Playlist  ·  me  ·  12 canciones"
        );
    }

    #[test]
    fn search_skips_nulls_and_empty_groups() {
        let payload = json!({
            "tracks": {"items": [{"type": "track", "uri": "spotify:track:1", "name": "a"}]},
            "playlists": {"items": [null]},
            "artists": {"items": []}
        });
        let groups = search_groups(&payload);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].kind, EntryKind::Track);
    }

    #[test]
    fn recent_is_deduplicated() {
        let track = json!({"type": "track", "uri": "spotify:track:1", "name": "a"});
        let payload = json!({"items": [{"track": track}, {"track": track}]});
        assert_eq!(recent(&payload, 10).len(), 1);
    }
}
