//! The library half of the popup: search, queue, playlists and podcasts,
//! the contents of whatever is opened, and the device picker.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use cosmic::widget::image::Handle;
use cosmic::widget::segmented_button::{Entity, SingleSelectModel};
use cosmic::{Action, Task};

use crate::art;
use crate::fl;
use crate::spotify::{
    self, Device, Entry, EntryKind, Item, ItemKind, SearchGroup, Session, Spotify,
};
use crate::window::{Message, Window, delayed};

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(350);
/// Recently played is fetched again after this, or as soon as the track changes.
const RECENT_REFRESH: Duration = Duration::from_secs(30);
const RECENT_KEEP: usize = 30;
const NOTICE_FOR: Duration = Duration::from_secs(3);
/// Thumbnails are small, but a long browsing session should not keep them all.
const MAX_THUMBS: usize = 600;
/// How many covers download at once. More than this stalls the popup.
const THUMB_PARALLEL: usize = 6;
/// About one library row, used to guess which covers are on screen.
const ROW_PX: f32 = 56.0;
const VISIBLE_ROWS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Search,
    Queue,
    Playlists,
    Podcasts,
}

#[derive(Clone, Debug, Default)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    fn from_result(result: Result<T, spotify::Error>) -> Self {
        match result {
            Ok(value) => Self::Ready(value),
            Err(error) => Self::Failed(error.to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Liked,
    Entry(Entry),
}

impl Source {
    fn key(&self) -> &str {
        match self {
            Self::Liked => "liked",
            Self::Entry(entry) => &entry.uri,
        }
    }
}

pub struct Detail {
    pub source: Source,
    pub items: Load<Vec<Entry>>,
}

pub struct Library {
    pub tabs: SingleSelectModel,
    pub tab: Tab,
    pub query: String,
    search_generation: u64,
    pub results: Load<Vec<SearchGroup>>,
    pub recent: Load<Vec<Entry>>,
    /// Tracks heard this session, newest first. Spotify's history omits
    /// whatever is still playing, so these stay at the top.
    heard: Vec<Entry>,
    recent_at: Option<std::time::Instant>,
    recent_generation: u64,
    /// The track changed, so play history may have a new row.
    recent_dirty: bool,
    pub queue: Load<Vec<Entry>>,
    queue_generation: u64,
    /// Tracks just removed, so a stale reload cannot put them back.
    dropped: Vec<(String, std::time::Instant)>,
    pub playlists: Load<Vec<Entry>>,
    pub liked_count: u64,
    pub shows: Load<Vec<Entry>>,
    pub detail: Option<Detail>,
    pub devices: Option<Load<Vec<Device>>>,
    pub thumbs: HashMap<String, Option<Handle>>,
    pub notice: Option<String>,
    /// Tracks just added to the queue, until the notice goes away.
    pub queued: HashSet<String>,
    notice_generation: u64,
    /// Scroll offset of each list, so closing the popup does not jump back up.
    scroll: HashMap<String, f32>,
    /// False while a saved offset is being put back, so the jump to the top
    /// is not stored over the real position.
    scroll_live: bool,
    thumb_waiting: VecDeque<String>,
    thumbs_inflight: usize,
}

impl Default for Library {
    fn default() -> Self {
        let tabs = SingleSelectModel::builder()
            .insert(|tab| tab.text(fl!("tab-search")).data(Tab::Search).activate())
            .insert(|tab| tab.text(fl!("tab-queue")).data(Tab::Queue))
            .insert(|tab| tab.text(fl!("tab-playlists")).data(Tab::Playlists))
            .insert(|tab| tab.text(fl!("tab-podcasts")).data(Tab::Podcasts))
            .build();
        Self {
            tabs,
            tab: Tab::Search,
            query: String::new(),
            search_generation: 0,
            results: Load::Idle,
            recent: Load::Idle,
            heard: Vec::new(),
            recent_at: None,
            recent_generation: 0,
            recent_dirty: false,
            queue: Load::Idle,
            queue_generation: 0,
            dropped: Vec::new(),
            playlists: Load::Idle,
            liked_count: 0,
            shows: Load::Idle,
            detail: None,
            devices: None,
            thumbs: HashMap::new(),
            notice: None,
            queued: HashSet::new(),
            notice_generation: 0,
            scroll: HashMap::new(),
            scroll_live: false,
            thumb_waiting: VecDeque::new(),
            thumbs_inflight: 0,
        }
    }
}

impl Library {
    /// Which list is on screen: an open playlist, or one of the tabs.
    fn scroll_key(&self) -> String {
        if let Some(detail) = &self.detail {
            format!("detail:{}", detail.source.key())
        } else {
            format!("tab:{}", self.tab.name())
        }
    }

    fn saved_scroll(&self) -> f32 {
        self.scroll.get(&self.scroll_key()).copied().unwrap_or(0.0)
    }

    pub(crate) fn mark_recent_dirty(&mut self) {
        self.recent_dirty = true;
    }

    /// Puts the track on screen at the top of recently played, right away.
    pub(crate) fn remember_item(&mut self, item: &Item) {
        let entry = Entry {
            kind: match item.kind {
                ItemKind::Track => EntryKind::Track,
                ItemKind::Episode => EntryKind::Episode,
            },
            id: item.id.clone(),
            uri: item.uri.clone(),
            name: item.name.clone(),
            detail: item.subtitle.clone(),
            parent_uri: item.parent_uri.clone(),
            duration_ms: item.duration_ms,
            art_url: item.art_url.clone(),
            playable: true,
        };
        self.heard.retain(|existing| existing.uri != entry.uri);
        self.heard.insert(0, entry);
        self.heard.truncate(RECENT_KEEP);
        let server = match &self.recent {
            Load::Ready(list) => list.clone(),
            _ => Vec::new(),
        };
        self.recent = Load::Ready(compose_recent(&self.heard, &server));
    }

    /// True when play history was never loaded, went stale, or the track changed.
    fn recent_needs_refresh(&self) -> bool {
        matches!(self.recent, Load::Idle | Load::Failed(_))
            || self.recent_dirty
            || self
                .recent_at
                .is_none_or(|at| at.elapsed() > RECENT_REFRESH)
    }

    /// Drops one copy of each track the user just removed, if Spotify still lists it.
    fn without_dropped(&mut self, mut entries: Vec<Entry>) -> Vec<Entry> {
        let now = std::time::Instant::now();
        self.dropped.retain(|(_, until)| *until > now);
        for (uri, _) in &self.dropped {
            if let Some(index) = entries.iter().position(|entry| entry.uri == *uri) {
                entries.remove(index);
            }
        }
        entries
    }
}

/// What should follow `uri`. Picked from the queue itself, the rows before it
/// were skipped; picked elsewhere, the whole queue still follows.
fn queue_after(upcoming: &[Entry], uri: &str, from_queue: bool) -> Vec<String> {
    let start = if from_queue {
        upcoming
            .iter()
            .position(|entry| entry.uri == uri)
            .map_or(0, |at| at + 1)
    } else {
        0
    };
    upcoming[start..]
        .iter()
        .filter(|entry| entry.uri != uri)
        .map(|entry| entry.uri.clone())
        .collect()
}

/// Heard tracks first, then Spotify's history, each URI once.
fn compose_recent(heard: &[Entry], server: &[Entry]) -> Vec<Entry> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for entry in heard.iter().chain(server.iter()) {
        if seen.insert(entry.uri.clone()) {
            out.push(entry.clone());
            if out.len() == RECENT_KEEP {
                break;
            }
        }
    }
    out
}

impl Tab {
    fn name(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Queue => "queue",
            Self::Playlists => "playlists",
            Self::Podcasts => "podcasts",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Browse {
    SelectTab(Entity),
    Query(String),
    RunSearch(u64),
    SubmitSearch,
    ClearSearch,
    SearchLoaded(u64, Result<Vec<SearchGroup>, spotify::Error>),
    RecentLoaded(u64, Result<Vec<Entry>, spotify::Error>),
    PlaylistsLoaded(Result<(Vec<Entry>, u64), spotify::Error>),
    QueueLoaded(u64, Result<Vec<Entry>, spotify::Error>),
    ShowsLoaded(Result<Vec<Entry>, spotify::Error>),
    /// A row was clicked: open a context, or play an item.
    Activate(Entry),
    /// Play a whole context without opening it.
    PlayContext(Entry),
    OpenLiked,
    DetailPage(String, Result<spotify::ListPage, spotify::Error>),
    CloseDetail,
    PlayDetail,
    Queue(Entry),
    Queued(String, String, Result<(), spotify::Error>),
    Dequeue(usize),
    Dequeued(String, Result<(), spotify::Error>),
    ThumbLoaded(String, Option<Handle>),
    Reload,
    ToggleDevices,
    DevicesLoaded(Result<Vec<Device>, spotify::Error>),
    Transfer(String),
    ClearNotice(u64),
}

fn detail_request(
    spotify: spotify::Spotify,
    source: Source,
    offset: usize,
) -> Task<Action<Message>> {
    let key = source.key().to_owned();
    Task::perform(
        async move {
            match &source {
                Source::Liked => spotify.liked_from(offset).await,
                Source::Entry(entry) => spotify.contents_from(entry, offset).await,
            }
        },
        move |result| browse(Browse::DetailPage(key, result)),
    )
}

fn browse(message: Browse) -> Action<Message> {
    Action::App(Message::Browse(message))
}

fn fetch<T, F, Fut>(
    spotify: Spotify,
    run: F,
    done: fn(Result<T, spotify::Error>) -> Browse,
) -> Task<Action<Message>>
where
    T: Send + 'static,
    F: FnOnce(Spotify) -> Fut,
    Fut: Future<Output = Result<T, spotify::Error>> + Send + 'static,
{
    Task::perform(run(spotify), move |result| browse(done(result)))
}

impl Window {
    #[allow(clippy::too_many_lines)]
    pub(crate) fn update_browse(&mut self, message: Browse) -> Task<Action<Message>> {
        match message {
            Browse::SelectTab(entity) => {
                self.library.tabs.activate(entity);
                if let Some(&tab) = self.library.tabs.data::<Tab>(entity) {
                    self.library.tab = tab;
                }
                self.library.detail = None;
                return Task::batch([self.ensure_library(), self.restore_library_scroll()]);
            }
            Browse::Query(query) => {
                self.library.query = query;
                self.library.search_generation += 1;
                if self.library.query.trim().is_empty() {
                    self.library.results = Load::Idle;
                    return self.ensure_library();
                }
                return delayed(
                    SEARCH_DEBOUNCE,
                    Message::Browse(Browse::RunSearch(self.library.search_generation)),
                );
            }
            Browse::SubmitSearch => {
                self.library.search_generation += 1;
                return self.run_search();
            }
            Browse::RunSearch(generation) => {
                if generation == self.library.search_generation {
                    return self.run_search();
                }
            }
            Browse::ClearSearch => {
                self.library.query.clear();
                self.library.search_generation += 1;
                self.library.results = Load::Idle;
                return self.ensure_library();
            }
            Browse::SearchLoaded(generation, result) => {
                if generation != self.library.search_generation {
                    return Task::none();
                }
                self.note_error(&result);
                self.library.results = Load::from_result(result);
                return self.enqueue_thumbs();
            }
            Browse::RecentLoaded(generation, result) => {
                if generation != self.library.recent_generation {
                    return Task::none();
                }
                self.library.recent_at = Some(std::time::Instant::now());
                self.note_error(&result);
                match result {
                    Ok(entries) => {
                        self.library.recent =
                            Load::Ready(compose_recent(&self.library.heard, &entries));
                    }
                    Err(error) => {
                        if !matches!(self.library.recent, Load::Ready(_)) {
                            self.library.recent = Load::Failed(error.to_string());
                        }
                    }
                }
                return self.enqueue_thumbs();
            }
            Browse::PlaylistsLoaded(result) => {
                self.note_error(&result);
                self.library.playlists = match result {
                    Ok((playlists, liked)) => {
                        self.library.liked_count = liked;
                        Load::Ready(playlists)
                    }
                    Err(error) => Load::Failed(error.to_string()),
                };
                return self.enqueue_thumbs();
            }
            Browse::QueueLoaded(generation, result) => {
                if generation != self.library.queue_generation {
                    return Task::none();
                }
                self.note_error(&result);
                let queue = match result {
                    Err(spotify::Error::NoDevice | spotify::Error::NotFound(_)) => {
                        Load::Ready(Vec::new())
                    }
                    Ok(entries) => Load::Ready(self.library.without_dropped(entries)),
                    Err(error) => Load::Failed(error.to_string()),
                };
                self.library.queue = queue;
                return self.enqueue_thumbs();
            }
            Browse::ShowsLoaded(result) => {
                self.note_error(&result);
                self.library.shows = Load::from_result(result);
                return self.enqueue_thumbs();
            }
            Browse::Activate(entry) => return self.activate(entry),
            Browse::PlayContext(entry) => {
                let uri = entry.uri;
                return self
                    .command(move |spotify| async move { spotify.play_context(&uri, None).await });
            }
            Browse::OpenLiked => return self.open(Source::Liked),
            Browse::DetailPage(key, result) => return self.detail_page(&key, result),
            Browse::CloseDetail => {
                self.library.detail = None;
                return self.restore_library_scroll();
            }
            Browse::PlayDetail => return self.play_detail(),
            Browse::Queue(entry) => {
                let Some(spotify) = self.client() else {
                    return Task::none();
                };
                let uri = entry.uri.clone();
                let name = entry.name.clone();
                return Task::perform(
                    async move {
                        let result = spotify.queue(&uri).await;
                        (name, uri, result)
                    },
                    |(name, uri, result)| browse(Browse::Queued(name, uri, result)),
                );
            }
            Browse::Dequeue(index) => return self.remove_from_queue(index),
            Browse::Dequeued(name, result) => {
                return match result {
                    Ok(()) => self.notify(fl!("queue-removed", track = name)),
                    Err(error) => {
                        self.library.queue = Load::Idle;
                        self.handle_api_error(&error);
                        if self.library.tab == Tab::Queue {
                            self.ensure_library()
                        } else {
                            Task::none()
                        }
                    }
                };
            }
            Browse::Queued(name, uri, result) => {
                return match result {
                    Ok(()) => {
                        self.library.dropped.retain(|(dropped, _)| dropped != &uri);
                        self.library.queued.insert(uri);
                        self.library.queue = Load::Idle;
                        let notice = self.notify(fl!("queue-queued", track = name));
                        let refresh = if self.library.tab == Tab::Queue {
                            self.ensure_library()
                        } else {
                            Task::none()
                        };
                        Task::batch([notice, refresh])
                    }
                    Err(error) => {
                        self.library.notice = None;
                        self.handle_api_error(&error);
                        Task::none()
                    }
                };
            }
            Browse::ThumbLoaded(url, handle) => {
                self.library.thumbs_inflight = self.library.thumbs_inflight.saturating_sub(1);
                // `None` stays in the map so a failed cover is not requested again.
                self.library.thumbs.insert(url, handle);
                return self.pump_thumbs();
            }
            Browse::Reload => {
                match self.library.tab {
                    Tab::Search if self.library.query.trim().is_empty() => {
                        self.library.recent = Load::Idle;
                    }
                    Tab::Search => {
                        self.library.search_generation += 1;
                        return Task::batch([self.run_search(), self.refresh_player()]);
                    }
                    Tab::Queue => self.library.queue = Load::Idle,
                    Tab::Playlists => self.library.playlists = Load::Idle,
                    Tab::Podcasts => self.library.shows = Load::Idle,
                }
                if let Some(detail) = self.library.detail.as_mut() {
                    detail.items = Load::Idle;
                }
                let detail = self
                    .library
                    .detail
                    .as_ref()
                    .map(|detail| detail.source.clone());
                let reload = match detail {
                    Some(source) => self.open(source),
                    None => self.ensure_library(),
                };
                return Task::batch([reload, self.refresh_player()]);
            }
            Browse::ToggleDevices => {
                if self.library.devices.take().is_some() {
                    return Task::none();
                }
                let Some(spotify) = self.client() else {
                    return Task::none();
                };
                self.library.devices = Some(Load::Loading);
                return fetch(
                    spotify,
                    |spotify| async move { spotify.devices().await },
                    Browse::DevicesLoaded,
                );
            }
            Browse::DevicesLoaded(result) => {
                self.note_error(&result);
                if self.library.devices.is_some() {
                    self.library.devices = Some(Load::from_result(result));
                }
            }
            Browse::Transfer(device_id) => {
                self.library.devices = None;
                return self.command(move |spotify| async move {
                    spotify.transfer(&device_id, true).await
                });
            }
            Browse::ClearNotice(generation) => {
                if generation == self.library.notice_generation {
                    self.library.notice = None;
                    self.library.queued.clear();
                }
            }
        }
        Task::none()
    }

    /// Loads whatever the visible tab needs and has not loaded yet.
    pub(crate) fn ensure_library(&mut self) -> Task<Action<Message>> {
        if !matches!(self.session, Session::Connected(_)) {
            return Task::none();
        }
        let Some(spotify) = self.client() else {
            return Task::none();
        };
        let library = &mut self.library;
        match library.tab {
            Tab::Search if library.query.trim().is_empty() => {
                if library.recent_needs_refresh() {
                    let idle = matches!(library.recent, Load::Idle | Load::Failed(_));
                    library.recent_generation = library.recent_generation.wrapping_add(1);
                    library.recent_dirty = false;
                    if idle {
                        library.recent = Load::Loading;
                    }
                    let generation = library.recent_generation;
                    return Task::perform(async move { spotify.recent().await }, move |result| {
                        browse(Browse::RecentLoaded(generation, result))
                    });
                }
            }
            Tab::Search => {}
            Tab::Queue => {
                if matches!(library.queue, Load::Idle) {
                    return self.refresh_queue();
                }
            }
            Tab::Playlists => {
                if matches!(library.playlists, Load::Idle) {
                    library.playlists = Load::Loading;
                    return fetch(
                        spotify,
                        |spotify| async move { spotify.playlists().await },
                        Browse::PlaylistsLoaded,
                    );
                }
            }
            Tab::Podcasts => {
                if matches!(library.shows, Load::Idle) {
                    library.shows = Load::Loading;
                    return fetch(
                        spotify,
                        |spotify| async move { spotify.shows().await },
                        Browse::ShowsLoaded,
                    );
                }
            }
        }
        Task::none()
    }

    /// Fetches the queue again, keeping a list already on screen until the new
    /// one arrives.
    pub(crate) fn refresh_queue(&mut self) -> Task<Action<Message>> {
        let Some(spotify) = self.client() else {
            return Task::none();
        };
        let library = &mut self.library;
        if !matches!(library.queue, Load::Ready(_)) {
            library.queue = Load::Loading;
        }
        library.queue_generation = library.queue_generation.wrapping_add(1);
        let generation = library.queue_generation;
        Task::perform(
            async move { spotify.playback_queue().await },
            move |result| browse(Browse::QueueLoaded(generation, result)),
        )
    }

    fn run_search(&mut self) -> Task<Action<Message>> {
        let query = self.library.query.trim().to_owned();
        let Some(spotify) = self.client().filter(|_| !query.is_empty()) else {
            return Task::none();
        };
        let generation = self.library.search_generation;
        self.library.results = Load::Loading;
        Task::perform(async move { spotify.search(&query).await }, move |result| {
            browse(Browse::SearchLoaded(generation, result))
        })
    }

    fn activate(&mut self, entry: Entry) -> Task<Action<Message>> {
        if entry.kind.is_item()
            && let Some(detail) = &self.library.detail
        {
            match &detail.source {
                Source::Entry(parent) => {
                    let (context, offset) = (parent.uri.clone(), entry.uri);
                    return self.command(move |spotify| async move {
                        spotify.play_context(&context, Some(&offset)).await
                    });
                }
                // Liked songs are not a context the Web API can start at an offset.
                Source::Liked => {
                    let uris = match &detail.items {
                        Load::Ready(items) => items
                            .iter()
                            .skip_while(|item| item.uri != entry.uri)
                            .map(|item| item.uri.clone())
                            .collect(),
                        _ => vec![entry.uri],
                    };
                    return self
                        .command(move |spotify| async move { spotify.play_uris(&uris).await });
                }
            }
        }
        if entry.kind.opens() {
            return self.open(Source::Entry(entry));
        }
        if entry.kind == EntryKind::Artist {
            return self.update_browse(Browse::PlayContext(entry));
        }
        self.play_before_queue(entry)
    }

    /// Plays a single track and then whatever the queue already had, so the
    /// next button does not drop into that track's album or stop.
    fn play_before_queue(&mut self, entry: Entry) -> Task<Action<Message>> {
        let from_queue = self.library.tab == Tab::Queue;
        let uri = entry.uri;
        let parent = entry.parent_uri;
        self.library.queue = Load::Idle;
        self.command(move |spotify| async move {
            let upcoming = spotify.playback_queue().await.unwrap_or_default();
            let after = queue_after(&upcoming, &uri, from_queue);
            if after.is_empty()
                && let Some(context) = parent
            {
                return spotify.play_context(&context, Some(&uri)).await;
            }
            let mut uris = Vec::with_capacity(after.len() + 1);
            uris.push(uri);
            uris.extend(after);
            spotify.play_uris(&uris).await
        })
    }

    fn open(&mut self, source: Source) -> Task<Action<Message>> {
        let Some(spotify) = self.client() else {
            return Task::none();
        };
        let key = source.key().to_owned();
        if let Some(detail) = &self.library.detail
            && detail.source.key() == key
            && matches!(detail.items, Load::Ready(_))
        {
            return self.enqueue_thumbs();
        }
        // The list is about to be replaced. Ignore the placeholder's scroll.
        self.library.scroll_live = false;
        self.library.detail = Some(Detail {
            source: source.clone(),
            items: Load::Loading,
        });
        detail_request(spotify, source, 0)
    }

    fn detail_page(
        &mut self,
        key: &str,
        result: Result<spotify::ListPage, spotify::Error>,
    ) -> Task<Action<Message>> {
        self.note_error(&result);
        let Some((first, follow)) = self.store_detail_page(key, result) else {
            return Task::none();
        };
        let Some(spotify) = self.client() else {
            return self.enqueue_thumbs();
        };
        let mut tasks = vec![self.enqueue_thumbs()];
        if first {
            tasks.push(self.restore_library_scroll());
        }
        if let Some((source, offset)) = follow {
            tasks.push(detail_request(spotify, source, offset));
        }
        Task::batch(tasks)
    }

    fn store_detail_page(
        &mut self,
        key: &str,
        result: Result<spotify::ListPage, spotify::Error>,
    ) -> Option<(bool, Option<(Source, usize)>)> {
        let detail = self
            .library
            .detail
            .as_mut()
            .filter(|detail| detail.source.key() == key)?;
        let first = !matches!(detail.items, Load::Ready(_));
        let follow = match result {
            Ok(page) => {
                let next = page.next;
                match &mut detail.items {
                    Load::Ready(items) => items.extend(page.entries),
                    _ => detail.items = Load::Ready(page.entries),
                }
                next.map(|offset| (detail.source.clone(), offset))
            }
            Err(error) => {
                if !matches!(detail.items, Load::Ready(_)) {
                    // Since February 2026 Spotify only lists tracks of playlists
                    // the user owns or collaborates on. A public one still plays.
                    let blocked = matches!(
                        (&error, &detail.source),
                        (spotify::Error::Forbidden(_), Source::Entry(entry))
                            if entry.kind == EntryKind::Playlist
                    );
                    detail.items = Load::Failed(if blocked {
                        fl!("playlist-forbidden")
                    } else {
                        error.to_string()
                    });
                }
                None
            }
        };
        Some((first, follow))
    }

    /// Spotify has no "remove from queue". Rebuild what will play, without that row.
    fn remove_from_queue(&mut self, index: usize) -> Task<Action<Message>> {
        let Load::Ready(items) = &self.library.queue else {
            return Task::none();
        };
        if index >= items.len() {
            return Task::none();
        }
        let name = items[index].name.clone();
        let removed = items[index].uri.clone();
        let mut uris = Vec::new();
        if let Some(current) = self.item().map(|item| item.uri.clone()) {
            uris.push(current);
        }
        uris.extend(
            items
                .iter()
                .enumerate()
                .filter(|(at, _)| *at != index)
                .map(|(_, entry)| entry.uri.clone()),
        );
        let was_playing = self.is_playing();
        let position = self.progress_ms();
        self.hold_progress();
        // Drop any queue snapshot that left before this removal.
        self.library.queue_generation = self.library.queue_generation.wrapping_add(1);
        self.library
            .dropped
            .push((removed, std::time::Instant::now() + Duration::from_secs(20)));
        if let Load::Ready(list) = &mut self.library.queue {
            list.remove(index);
        }
        let Some(spotify) = self.client() else {
            return Task::none();
        };
        Task::perform(
            async move {
                if !uris.is_empty() {
                    // One request, already at the current position, so the bar
                    // does not jump to the start and then seek forward.
                    spotify.play_uris_at(&uris, position).await?;
                    if !was_playing {
                        let _ = spotify.pause().await;
                    }
                }
                Ok(())
            },
            move |result| browse(Browse::Dequeued(name, result)),
        )
    }

    fn play_detail(&mut self) -> Task<Action<Message>> {
        let Some(detail) = &self.library.detail else {
            return Task::none();
        };
        match &detail.source {
            Source::Entry(entry) => {
                let uri = entry.uri.clone();
                self.command(move |spotify| async move { spotify.play_context(&uri, None).await })
            }
            Source::Liked => {
                let uris: Vec<String> = match &detail.items {
                    Load::Ready(items) => items.iter().map(|item| item.uri.clone()).collect(),
                    _ => return Task::none(),
                };
                self.command(move |spotify| async move { spotify.play_uris(&uris).await })
            }
        }
    }

    pub(crate) fn remember_library_scroll(&mut self, offset: f32) {
        if !self.library.scroll_live {
            return;
        }
        let key = self.library.scroll_key();
        if offset < 1.0 {
            self.library.scroll.remove(&key);
        } else {
            self.library.scroll.insert(key, offset);
        }
    }

    /// The popup is a new window every time it opens, so the list has to be
    /// scrolled again once that window exists.
    pub(crate) fn restore_library_scroll(&mut self) -> Task<Action<Message>> {
        self.library.scroll_live = false;
        let offset = self.library.saved_scroll();
        delayed(
            Duration::from_millis(80),
            Message::ApplyLibraryScroll(offset),
        )
    }

    pub(crate) fn apply_library_scroll(&mut self, offset: f32) -> Task<Action<Message>> {
        self.library.scroll_live = true;
        let thumbs = self.enqueue_thumbs();
        if offset < 1.0 {
            return thumbs;
        }
        let scroll = cosmic::iced::widget::scrollable::scroll_to(
            cosmic::widget::Id::new(crate::ui::LIBRARY_SCROLL),
            cosmic::iced::widget::scrollable::AbsoluteOffset {
                x: Some(0.0),
                y: Some(offset),
            },
        );
        Task::batch([scroll, thumbs])
    }

    pub(crate) fn notify(&mut self, text: String) -> Task<Action<Message>> {
        self.library.notice = Some(text);
        self.library.notice_generation += 1;
        delayed(
            NOTICE_FOR,
            Message::Browse(Browse::ClearNotice(self.library.notice_generation)),
        )
    }

    fn note_error<T>(&mut self, result: &Result<T, spotify::Error>) {
        if let Err(error) = result
            && matches!(error, spotify::Error::Reauth | spotify::Error::SignedOut)
        {
            self.handle_api_error(error);
        }
    }

    /// The entries the popup currently lists.
    pub(crate) fn visible_entries(&self) -> Vec<&Entry> {
        fn ready(load: &Load<Vec<Entry>>) -> Vec<&Entry> {
            match load {
                Load::Ready(list) => list.iter().collect(),
                _ => Vec::new(),
            }
        }
        let library = &self.library;
        if let Some(detail) = &library.detail {
            return ready(&detail.items);
        }
        match library.tab {
            Tab::Search if library.query.trim().is_empty() => ready(&library.recent),
            Tab::Search => match &library.results {
                Load::Ready(groups) => groups.iter().flat_map(|group| &group.entries).collect(),
                _ => Vec::new(),
            },
            Tab::Queue => ready(&library.queue),
            Tab::Playlists => ready(&library.playlists),
            Tab::Podcasts => ready(&library.shows),
        }
    }

    /// Queues covers for the rows on screen first, then the rest of the list.
    pub(crate) fn enqueue_thumbs(&mut self) -> Task<Action<Message>> {
        let urls = self.ordered_art_urls();
        if self.library.thumbs.len() + urls.len() > MAX_THUMBS {
            let keep = &urls;
            self.library
                .thumbs
                .retain(|url, _| keep.iter().any(|wanted| wanted == url));
        }
        for url in urls.into_iter().rev() {
            let queued = self
                .library
                .thumb_waiting
                .iter()
                .any(|waiting| waiting == &url);
            if self.library.thumbs.contains_key(&url) || queued {
                continue;
            }
            self.library.thumb_waiting.push_front(url);
        }
        self.pump_thumbs()
    }

    fn ordered_art_urls(&self) -> Vec<String> {
        let entries = self.visible_entries();
        let count = entries.len();
        if count == 0 {
            return Vec::new();
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let first = (self.library.saved_scroll() / ROW_PX) as usize;
        let first = first.min(count - 1);
        let last = (first + VISIBLE_ROWS).min(count);
        let mut urls = Vec::new();
        let mut push = |entry: &Entry| {
            let Some(url) = &entry.art_url else {
                return;
            };
            if !urls.contains(url) {
                urls.push(url.clone());
            }
        };
        for entry in &entries[first..last] {
            push(entry);
        }
        for (index, entry) in entries.iter().enumerate() {
            if !(first..last).contains(&index) {
                push(entry);
            }
        }
        urls
    }

    fn pump_thumbs(&mut self) -> Task<Action<Message>> {
        let Some(spotify) = self.client() else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        while self.library.thumbs_inflight < THUMB_PARALLEL {
            let Some(url) = self.library.thumb_waiting.pop_front() else {
                break;
            };
            if self.library.thumbs.contains_key(&url) {
                continue;
            }
            self.library.thumbs.insert(url.clone(), None);
            self.library.thumbs_inflight += 1;
            let spotify = spotify.clone();
            tasks.push(Task::perform(
                async move {
                    let handle = match art::fetch(url.clone(), spotify).await {
                        Ok(bytes) => art::decode_thumb(&bytes).ok(),
                        Err(_) => None,
                    };
                    (url, handle)
                },
                |(url, handle)| browse(Browse::ThumbLoaded(url, handle)),
            ));
        }
        Task::batch(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spotify::{Entry, EntryKind};

    #[test]
    fn recent_refreshes_once_it_is_stale_or_the_track_moved_on() {
        let mut library = Library::default();
        assert!(library.recent_needs_refresh());
        library.recent = Load::Ready(Vec::new());
        library.recent_at = Some(std::time::Instant::now());
        assert!(!library.recent_needs_refresh());
        library.recent_dirty = true;
        assert!(library.recent_needs_refresh());
    }

    #[test]
    fn a_track_just_played_stays_ahead_of_spotifys_history() {
        let mut library = Library::default();
        library.remember_item(&played("spotify:track:new", "New"));
        let Load::Ready(list) = &library.recent else {
            panic!("recent list");
        };
        assert_eq!(list[0].name, "New");
        let merged = compose_recent(&library.heard, &[entry("spotify:track:old")]);
        assert_eq!(merged[0].uri, "spotify:track:new");
        assert_eq!(merged[1].uri, "spotify:track:old");
    }

    #[test]
    fn a_picked_track_is_followed_by_the_queue() {
        let queue = [entry("a"), entry("b"), entry("c")];
        assert_eq!(queue_after(&queue, "x", false), ["a", "b", "c"]);
        assert_eq!(queue_after(&queue, "b", false), ["a", "c"]);
        assert_eq!(queue_after(&queue, "b", true), ["c"]);
        assert!(queue_after(&[], "x", false).is_empty());
    }

    fn played(uri: &str, name: &str) -> crate::spotify::Item {
        crate::spotify::Item {
            kind: crate::spotify::ItemKind::Track,
            id: String::new(),
            uri: uri.to_owned(),
            name: name.to_owned(),
            subtitle: String::new(),
            album: String::new(),
            parent_uri: None,
            duration_ms: 0,
            art_url: None,
        }
    }

    #[test]
    fn each_list_keeps_its_own_scroll() {
        let mut library = Library::default();
        assert_eq!(library.scroll_key(), "tab:search");
        library.tab = Tab::Playlists;
        library.scroll.insert(library.scroll_key(), 240.0);
        library.detail = Some(Detail {
            source: Source::Liked,
            items: Load::Idle,
        });
        assert_eq!(library.scroll_key(), "detail:liked");
        assert!(library.saved_scroll().abs() < f32::EPSILON);
        library.detail = None;
        assert!((library.saved_scroll() - 240.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_removed_track_stays_out_of_a_stale_queue() {
        let mut library = Library::default();
        library.dropped.push((
            "spotify:track:removed".into(),
            std::time::Instant::now() + std::time::Duration::from_secs(20),
        ));
        let stale = vec![
            entry("spotify:track:other"),
            entry("spotify:track:removed"),
            entry("spotify:track:removed"),
        ];
        let kept = library.without_dropped(stale);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].uri, "spotify:track:other");
        assert_eq!(kept[1].uri, "spotify:track:removed");
    }

    fn entry(uri: &str) -> Entry {
        Entry {
            kind: EntryKind::Track,
            id: String::new(),
            uri: uri.to_owned(),
            name: String::new(),
            detail: String::new(),
            parent_uri: None,
            duration_ms: 0,
            art_url: None,
            playable: true,
        }
    }
}
