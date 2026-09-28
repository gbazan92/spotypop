use std::sync::{Arc, Mutex, MutexGuard};

use reqwest::Method;
use reqwest::header::{CONTENT_LENGTH, RETRY_AFTER};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::auth::{self, PendingLogin, now};
use super::http::{self, Http};
use super::library;
use super::store::{Store, StoredAuth};
use super::types::{RawDevices, RawMe, RawPlayer, check_id, check_uri};
use super::{Device, Entry, EntryKind, Error, PlayerState, Repeat, SearchGroup, User};

const API: &str = "https://api.spotify.com/v1";
/// Spotify caps search at 10 results per type.
const SEARCH_LIMIT: u8 = 6;
const RECENT_LIMIT: usize = 30;
const LIBRARY_LIMIT: usize = 100;
const LIST_LIMIT: usize = 200;
const PAGE_SIZE: usize = 50;
const MAX_PLAY_URIS: usize = 100;
const LOCAL_DEVICE_POLLS: u32 = 8;
const LOCAL_DEVICE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
/// Refresh a little early so a request never leaves with a token about to expire.
const EXPIRY_MARGIN_SECS: u64 = 45;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Session {
    SignedOut,
    NeedsReauth,
    Connected(User),
}

/// One page of a collection. `next` is the offset of the following page.
#[derive(Clone, Debug)]
pub struct ListPage {
    pub entries: Vec<Entry>,
    pub next: Option<usize>,
}

/// Cheap to clone: every clone shares the session and the HTTP connection pool.
#[derive(Clone, Debug)]
pub struct Spotify {
    http: Http,
    store: Store,
    auth: Arc<Mutex<Option<StoredAuth>>>,
    refresh_lock: Arc<tokio::sync::Mutex<()>>,
    local_device: Arc<Mutex<Option<String>>>,
}

impl Spotify {
    pub fn new(store: Store) -> Result<Self, Error> {
        let auth = store.load()?;
        Ok(Self {
            http: Http::new()?,
            store,
            auth: Arc::new(Mutex::new(auth)),
            refresh_lock: Arc::new(tokio::sync::Mutex::new(())),
            local_device: Arc::new(Mutex::new(None)),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Option<StoredAuth>> {
        self.auth
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn session(&self) -> Session {
        match self.lock().as_ref() {
            Some(auth) if auth.needs_reauth => Session::NeedsReauth,
            Some(auth) if !auth.refresh_token.is_empty() => {
                Session::Connected(auth.user.clone().unwrap_or_default())
            }
            _ => Session::SignedOut,
        }
    }

    pub fn begin_login(client_id: &str, port: u16) -> Result<PendingLogin, Error> {
        auth::begin_login(client_id, port)
    }

    pub async fn complete_login(&self, pending: PendingLogin) -> Result<User, Error> {
        let mut fresh = auth::finish_login(&self.http, pending).await?;
        self.store.save(&fresh)?;
        *self.lock() = Some(fresh.clone());

        // The login itself succeeded; a failed profile lookup only costs the name.
        if let Ok(user) = self.me().await {
            fresh = self.lock().clone().unwrap_or(fresh);
            fresh.user = Some(user);
            self.store.save(&fresh)?;
            *self.lock() = Some(fresh);
        }
        Ok(self
            .lock()
            .as_ref()
            .and_then(|auth| auth.user.clone())
            .unwrap_or_default())
    }

    pub fn logout(&self) -> Result<(), Error> {
        *self.lock() = None;
        self.store.clear()
    }

    fn valid_token(&self) -> Result<Option<String>, Error> {
        let guard = self.lock();
        let auth = guard.as_ref().ok_or(Error::SignedOut)?;
        if auth.refresh_token.is_empty() {
            return Err(Error::SignedOut);
        }
        if auth.needs_reauth {
            return Err(Error::Reauth);
        }
        let fresh = !auth.access_token.is_empty() && now() + EXPIRY_MARGIN_SECS < auth.expires_at;
        Ok(fresh.then(|| auth.access_token.clone()))
    }

    async fn access_token(&self, force_refresh: bool) -> Result<String, Error> {
        if !force_refresh && let Some(token) = self.valid_token()? {
            return Ok(token);
        }
        // Spotify rotates refresh tokens, so two concurrent refreshes would
        // race and one of them would store a token that is already dead.
        let _guard = self.refresh_lock.lock().await;
        if !force_refresh && let Some(token) = self.valid_token()? {
            return Ok(token);
        }
        let snapshot = self.lock().clone().ok_or(Error::SignedOut)?;

        match auth::refresh(&self.http, &snapshot).await {
            Ok(tokens) => {
                let mut updated = snapshot;
                tokens.apply(&mut updated);
                let token = updated.access_token.clone();
                self.store.save(&updated)?;
                *self.lock() = Some(updated);
                Ok(token)
            }
            Err(Error::Reauth) => {
                let mut updated = snapshot;
                updated.needs_reauth = true;
                let _ = self.store.save(&updated);
                *self.lock() = Some(updated);
                Err(Error::Reauth)
            }
            Err(error) => Err(error),
        }
    }

    /// One Web API call; `None` for an empty (204) reply. A 401 gets one forced
    /// token refresh before it is reported.
    async fn call(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<Option<Vec<u8>>, Error> {
        let mut retried = false;
        loop {
            let token = self.access_token(retried).await?;
            let mut request = self
                .http
                .client()
                .request(method.clone(), format!("{API}{path}"))
                .bearer_auth(token);
            if !query.is_empty() {
                request = request.query(query);
            }
            request = match body {
                Some(body) => request.json(body),
                // Spotify answers 411 to a body-less PUT/POST without it.
                None if method == Method::PUT || method == Method::POST => {
                    request.header(CONTENT_LENGTH, "0")
                }
                None => request,
            };

            let response = request.send().await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse().ok());
            let limit = if status.is_success() {
                http::API_MAX_BYTES
            } else {
                http::ERROR_MAX_BYTES
            };
            let bytes = http::read_capped(response, limit).await?;

            if status.is_success() {
                return Ok((!bytes.iter().all(u8::is_ascii_whitespace)).then_some(bytes));
            }
            if status.as_u16() == 401 && !retried {
                retried = true;
                continue;
            }
            return Err(Error::from_response(status.as_u16(), &bytes, retry_after));
        }
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Option<T>, Error> {
        match self.call(Method::GET, path, query, None).await? {
            None => Ok(None),
            Some(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| Error::Network(format!("respuesta inesperada: {error}"))),
        }
    }

    async fn command(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<(), Error> {
        self.call(method, path, query, body).await.map(|_| ())
    }

    pub async fn me(&self) -> Result<User, Error> {
        let raw: Option<RawMe> = self.get("/me", &[]).await?;
        raw.map(User::from)
            .ok_or_else(|| Error::Network("empty profile".into()))
    }

    /// `None` when nothing is playing on any device.
    pub async fn player(&self) -> Result<Option<PlayerState>, Error> {
        let raw: Option<RawPlayer> = self
            .get("/me/player", &[("additional_types", "track,episode")])
            .await?;
        Ok(raw.map(PlayerState::from))
    }

    pub async fn devices(&self) -> Result<Vec<Device>, Error> {
        let raw: Option<RawDevices> = self.get("/me/player/devices", &[]).await?;
        Ok(raw
            .map(|raw| raw.devices.into_iter().map(Device::from).collect())
            .unwrap_or_default())
    }

    /// Name of this computer's receiver, preferred whenever nothing is playing.
    pub fn set_local_device(&self, name: Option<String>) {
        *self
            .local_device
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = name;
    }

    fn local_device(&self) -> Option<String> {
        self.local_device
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The local receiver takes a few seconds to show up after it starts, so
    /// it is polled for a while before settling for any other device.
    async fn fallback_device(&self) -> Result<String, Error> {
        let local = self.local_device();
        let attempts = if local.is_some() {
            LOCAL_DEVICE_POLLS
        } else {
            1
        };
        let mut devices = Vec::new();
        for attempt in 0..attempts {
            if attempt > 0 {
                tokio::time::sleep(LOCAL_DEVICE_POLL_INTERVAL).await;
            }
            devices = self.devices().await?;
            let found = local.as_deref().and_then(|name| {
                devices
                    .iter()
                    .find(|device| device.name == name && !device.is_restricted)
            });
            if let Some(id) = found.and_then(|device| device.id.clone()) {
                return Ok(id);
            }
        }
        devices
            .iter()
            .find(|device| device.is_active)
            .or(devices.first())
            .and_then(|device| device.id.clone())
            .ok_or(Error::NoDevice)
    }

    /// With no active device, retries on this computer's receiver or else the
    /// first device Spotify Connect knows about.
    async fn with_device(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<(), Error> {
        match self.command(method.clone(), path, query, body).await {
            Err(Error::NoDevice) => {
                let target = self.fallback_device().await?;
                let mut query = query.to_vec();
                query.push(("device_id", &target));
                self.command(method, path, &query, body).await
            }
            other => other,
        }
    }

    /// Resumes; with nothing active, moves the last session to a device, which
    /// Spotify handles more reliably than a bare play aimed at an idle one.
    pub async fn play(&self) -> Result<(), Error> {
        match self
            .command(Method::PUT, "/me/player/play", &[], None)
            .await
        {
            Err(Error::NoDevice) => {
                let target = self.fallback_device().await?;
                self.transfer(&target, true).await
            }
            other => other,
        }
    }

    /// Starts an album, playlist, artist, show or audiobook, optionally at one of its items.
    pub async fn play_context(&self, context: &str, offset: Option<&str>) -> Result<(), Error> {
        let mut body = json!({ "context_uri": check_uri(context)? });
        if let Some(offset) = offset {
            body["offset"] = json!({ "uri": check_uri(offset)? });
        }
        match self
            .with_device(Method::PUT, "/me/player/play", &[], Some(&body))
            .await
        {
            // Some contexts (audiobooks, now and then shows) refuse an offset;
            // the chosen item alone is better than nothing.
            Err(Error::Http { status, .. }) if offset.is_some() && (400..500).contains(&status) => {
                self.play_uris(&[offset.unwrap_or_default().to_owned()])
                    .await
            }
            other => other,
        }
    }

    pub async fn play_uris(&self, uris: &[String]) -> Result<(), Error> {
        self.play_uris_at(uris, 0).await
    }

    /// Same as [`play_uris`], starting the first track at `position_ms`.
    pub async fn play_uris_at(&self, uris: &[String], position_ms: u64) -> Result<(), Error> {
        let uris = uris
            .iter()
            .take(MAX_PLAY_URIS)
            .map(|uri| check_uri(uri))
            .collect::<Result<Vec<_>, _>>()?;
        if uris.is_empty() {
            return Ok(());
        }
        let mut body = json!({ "uris": uris });
        if position_ms > 0 {
            body["position_ms"] = json!(position_ms);
        }
        self.with_device(Method::PUT, "/me/player/play", &[], Some(&body))
            .await
    }

    /// What will play after the current track. An idle player has no queue.
    pub async fn playback_queue(&self) -> Result<Vec<Entry>, Error> {
        let payload: Option<Value> = self.get("/me/player/queue", &[]).await?;
        Ok(payload
            .and_then(|payload| payload.get("queue").cloned())
            .map(|queue| {
                let page = json!({ "items": queue });
                library::entries(&page, None, None)
            })
            .unwrap_or_default())
    }

    pub async fn queue(&self, uri: &str) -> Result<(), Error> {
        let uri = check_uri(uri)?;
        self.with_device(Method::POST, "/me/player/queue", &[("uri", uri)], None)
            .await
    }

    pub async fn transfer(&self, device_id: &str, play: bool) -> Result<(), Error> {
        let body = json!({ "device_ids": [check_id(device_id)?], "play": play });
        self.command(Method::PUT, "/me/player", &[], Some(&body))
            .await
    }

    /// Up to `limit` raw pages of a paginated collection.
    async fn pages(
        &self,
        path: &str,
        query: &[(&str, &str)],
        page: usize,
        limit: usize,
    ) -> Result<Vec<Value>, Error> {
        let page_size = page.to_string();
        let mut pages = Vec::new();
        let mut offset = 0;
        while offset < limit {
            let offset_text = offset.to_string();
            let mut window = query.to_vec();
            window.push(("limit", &page_size));
            window.push(("offset", &offset_text));
            let Some(body) = self.get::<Value>(path, &window).await? else {
                break;
            };
            let got = body
                .get("items")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            let more = body.get("next").is_some_and(|next| !next.is_null());
            pages.push(body);
            if got < page || !more {
                break;
            }
            offset += got;
        }
        Ok(pages)
    }

    async fn collection(
        &self,
        path: &str,
        query: &[(&str, &str)],
        envelope: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Entry>, Error> {
        let page = limit.min(50);
        Ok(self
            .pages(path, query, page, limit)
            .await?
            .iter()
            .flat_map(|page| library::entries(page, envelope, None))
            .take(limit)
            .collect())
    }

    pub async fn search(&self, query: &str) -> Result<Vec<SearchGroup>, Error> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let types = library::SEARCH_KINDS
            .iter()
            .map(|kind| kind.search_type())
            .collect::<Vec<_>>()
            .join(",");
        let limit = SEARCH_LIMIT.to_string();
        let payload: Option<Value> = self
            .get(
                "/search",
                &[("q", query), ("type", &types), ("limit", &limit)],
            )
            .await?;
        Ok(payload
            .map(|payload| library::search_groups(&payload))
            .unwrap_or_default())
    }

    pub async fn recent(&self) -> Result<Vec<Entry>, Error> {
        let payload: Option<Value> = self
            .get("/me/player/recently-played", &[("limit", "50")])
            .await?;
        Ok(payload
            .map(|payload| library::recent(&payload, RECENT_LIMIT))
            .unwrap_or_default())
    }

    /// The user's playlists and how many songs they have liked.
    pub async fn playlists(&self) -> Result<(Vec<Entry>, u64), Error> {
        let playlists = self
            .collection("/me/playlists", &[], None, LIBRARY_LIMIT)
            .await?;
        let liked = self
            .get::<Value>("/me/tracks", &[("limit", "1")])
            .await
            .ok()
            .flatten()
            .and_then(|page| page.get("total").and_then(Value::as_u64))
            .unwrap_or_default();
        Ok((playlists, liked))
    }

    pub async fn shows(&self) -> Result<Vec<Entry>, Error> {
        self.collection("/me/shows", &[], Some("show"), LIBRARY_LIMIT)
            .await
    }

    /// One page of a paged collection, plus the offset of the following page.
    pub async fn list_page(
        &self,
        path: &str,
        query: &[(&str, &str)],
        envelope: Option<&str>,
        offset: usize,
    ) -> Result<ListPage, Error> {
        let limit = PAGE_SIZE.to_string();
        let offset_text = offset.to_string();
        let mut window = query.to_vec();
        window.push(("limit", limit.as_str()));
        window.push(("offset", offset_text.as_str()));
        let Some(body) = self.get::<Value>(path, &window).await? else {
            return Ok(ListPage {
                entries: Vec::new(),
                next: None,
            });
        };
        let got = body
            .get("items")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let entries = library::entries(&body, envelope, None);
        let more = got > 0 && body.get("next").is_some_and(|next| !next.is_null());
        let next = more
            .then_some(offset + got)
            .filter(|next| *next < LIST_LIMIT);
        Ok(ListPage { entries, next })
    }

    /// Liked songs, one page at a time. `next` is the offset to ask for afterwards.
    pub async fn liked_from(&self, offset: usize) -> Result<ListPage, Error> {
        self.list_page("/me/tracks", &[], Some("track"), offset)
            .await
    }

    /// One page of whatever is inside an album, playlist, show or audiobook.
    pub async fn contents_from(&self, parent: &Entry, offset: usize) -> Result<ListPage, Error> {
        let id = check_id(&parent.id)?;
        match parent.kind {
            EntryKind::Playlist => {
                self.list_page(
                    &format!("/playlists/{id}/items"),
                    &[("additional_types", "track,episode")],
                    Some("item"),
                    offset,
                )
                .await
            }
            _ if offset == 0 => {
                let entries = self.embedded_contents(parent).await?;
                Ok(ListPage {
                    entries,
                    next: None,
                })
            }
            _ => Ok(ListPage {
                entries: Vec::new(),
                next: None,
            }),
        }
    }

    async fn embedded_contents(&self, parent: &Entry) -> Result<Vec<Entry>, Error> {
        let id = check_id(&parent.id)?;
        let (path, inner) = match parent.kind {
            // These objects carry their first page of contents inline.
            EntryKind::Album => (format!("/albums/{id}"), "tracks"),
            EntryKind::Show => (format!("/shows/{id}"), "episodes"),
            EntryKind::Audiobook => (format!("/audiobooks/{id}"), "chapters"),
            _ => return Ok(Vec::new()),
        };
        let owner: Option<Value> = self.get(&path, &[]).await?;
        Ok(owner
            .map(|owner| {
                owner
                    .get(inner)
                    .map(|page| library::entries(page, None, Some(&owner)))
                    .unwrap_or_default()
            })
            .unwrap_or_default())
    }

    pub async fn pause(&self) -> Result<(), Error> {
        self.command(Method::PUT, "/me/player/pause", &[], None)
            .await
    }

    pub async fn next(&self) -> Result<(), Error> {
        self.command(Method::POST, "/me/player/next", &[], None)
            .await
    }

    pub async fn previous(&self) -> Result<(), Error> {
        self.command(Method::POST, "/me/player/previous", &[], None)
            .await
    }

    pub async fn seek(&self, position_ms: u64) -> Result<(), Error> {
        let position = position_ms.to_string();
        self.command(
            Method::PUT,
            "/me/player/seek",
            &[("position_ms", &position)],
            None,
        )
        .await
    }

    pub async fn set_volume(&self, percent: u8) -> Result<(), Error> {
        let percent = percent.min(100).to_string();
        self.command(
            Method::PUT,
            "/me/player/volume",
            &[("volume_percent", &percent)],
            None,
        )
        .await
    }

    pub async fn set_shuffle(&self, on: bool) -> Result<(), Error> {
        let state = if on { "true" } else { "false" };
        self.command(Method::PUT, "/me/player/shuffle", &[("state", state)], None)
            .await
    }

    pub async fn set_repeat(&self, mode: Repeat) -> Result<(), Error> {
        self.command(
            Method::PUT,
            "/me/player/repeat",
            &[("state", mode.as_str())],
            None,
        )
        .await
    }

    pub async fn is_saved(&self, uri: &str) -> Result<bool, Error> {
        let uri = check_uri(uri)?;
        let flags: Option<Vec<bool>> = self.get("/me/library/contains", &[("uris", uri)]).await?;
        Ok(flags
            .and_then(|flags| flags.first().copied())
            .unwrap_or(false))
    }

    // The 2026 library endpoints take the URIs as a query parameter; a JSON
    // body is answered with "Missing required field: uris".
    pub async fn save(&self, uri: &str) -> Result<(), Error> {
        let uri = check_uri(uri)?;
        self.command(Method::PUT, "/me/library", &[("uris", uri)], None)
            .await
    }

    pub async fn unsave(&self, uri: &str) -> Result<(), Error> {
        let uri = check_uri(uri)?;
        self.command(Method::DELETE, "/me/library", &[("uris", uri)], None)
            .await
    }

    /// Cover art bytes. Fetched without the bearer token and only from Spotify's image CDNs.
    pub async fn artwork(&self, url: &str) -> Result<Vec<u8>, Error> {
        if !art_url_allowed(url) {
            return Err(Error::BadArgument(format!(
                "cover outside Spotify's CDN: {url:.60}"
            )));
        }
        let response = self.http.client().get(url).send().await?;
        if !response.status().is_success() {
            return Err(Error::Http {
                status: response.status().as_u16(),
                message: "could not download the cover".into(),
            });
        }
        http::read_capped(response, ART_MAX_BYTES).await
    }
}

const ART_MAX_BYTES: usize = 8 * 1024 * 1024;

fn art_url_allowed(url: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    let spotify_cdn = ["scdn.co", "spotifycdn.com"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|port| port == 443)
        && spotify_cdn
}

#[cfg(test)]
mod tests {
    use super::art_url_allowed;

    #[test]
    fn only_spotify_image_cdns_are_fetched() {
        assert!(art_url_allowed("https://i.scdn.co/image/ab67616d00001e02"));
        assert!(art_url_allowed(
            "https://image-cdn-ak.spotifycdn.com/image/x"
        ));
        assert!(!art_url_allowed("http://i.scdn.co/image/x"));
        assert!(!art_url_allowed("https://evilscdn.co/image/x"));
        assert!(!art_url_allowed("https://i.scdn.co.evil.com/x"));
        assert!(!art_url_allowed("https://user@i.scdn.co/x"));
        assert!(!art_url_allowed("https://i.scdn.co:8443/x"));
    }
}
