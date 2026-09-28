use std::future::Future;
use std::time::{Duration, Instant};

use cosmic::app::Core;
use cosmic::applet::cosmic_panel_config::PanelAnchor;
use cosmic::applet::token::subscription::{
    TokenRequest, TokenUpdate, activation_token_subscription,
};
use cosmic::cctk::sctk::reexports::calloop::channel::Sender;
use cosmic::cctk::sctk::reexports::protocols::xdg::shell::client::xdg_positioner::{
    Anchor, Gravity,
};
use cosmic::cosmic_config::{self, Config};
use cosmic::iced::mouse::ScrollDelta;
use cosmic::iced::task::Handle;
use cosmic::iced::window::Id;
use cosmic::iced::{Limits, Subscription, time};
use cosmic::surface::action::{LiveSettings, app_popup, destroy_popup};
use cosmic::{Action, Element, Task};

use crate::art::{self, Artwork};
use crate::browse::{Browse, Library};
use crate::config::{self, AppConfig, PanelLook};
use crate::fl;
use crate::spotify::{self, Item, PlayerState, Repeat, Session, Spotify, Store, User};
use crate::{browser, player, ui};

pub const APP_ID: &str = "io.github.gbazan92.SpotyPop";
pub const DASHBOARD_URL: &str = "https://developer.spotify.com/dashboard";
const STATE_DIR: &str = "spotypop";

const VOLUME_STEP: i16 = 5;
/// Spotify takes a moment before a command shows up in `/me/player`.
const SETTLE: Duration = Duration::from_millis(700);
const VOLUME_DEBOUNCE: Duration = Duration::from_millis(250);
/// How long the slider keeps the value the user chose while Spotify catches up.
const VOLUME_HOLD: Duration = Duration::from_secs(3);
/// Same wait for play, pause, shuffle and the other controls.
const CONTROL_HOLD: Duration = Duration::from_secs(3);
/// Touchpads report pixels; this many make one wheel notch.
const PIXELS_PER_NOTCH: f32 = 40.0;

/// Marks the activation-token request for the receiver's authorization; URLs
/// always carry a scheme, so it cannot be mistaken for one.
const PLAYER_LOGIN: &str = "player-login";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Player,
    Settings,
}

/// Whether this computer can play on its own through the local receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Playback {
    /// The receiver binary is not installed next to the applet.
    Missing,
    NeedsLogin,
    Authorizing,
    Ready,
}

pub struct Window {
    pub(crate) core: Core,
    popup: Option<Id>,
    /// Last size of the panel button, so the popup can be anchored to it.
    panel_size: Option<(u32, u32)>,
    pub(crate) view: View,
    config_handler: Option<Config>,
    pub(crate) config: AppConfig,
    pub(crate) client_id_draft: String,
    spotify: Result<Spotify, spotify::Error>,
    pub(crate) session: Session,
    login: Option<Handle>,
    pub(crate) player: Option<PlayerState>,
    fetched_at: Instant,
    pub(crate) loaded: bool,
    /// The last thing that played, shown dimmed while nothing plays.
    pub(crate) last_item: Option<Item>,
    pub(crate) error: Option<String>,
    pub(crate) saved: Option<bool>,
    saved_uri: String,
    pub(crate) artwork: Option<Artwork>,
    art_pending: Option<String>,
    pub(crate) seek_drag: Option<f64>,
    volume_generation: u64,
    /// Value shown on the slider until a poll reports the same number.
    volume_held: Option<(u8, Instant)>,
    /// Controls kept as the user left them until a poll agrees.
    playback_held: Option<PlaybackHold>,
    /// Progress kept on screen while a queue edit restarts playback underneath.
    progress_held: Option<(u64, Instant, bool)>,
    volume_before_mute: Option<u8>,
    scroll_notches: f32,
    pub(crate) copied: bool,
    token_tx: Option<Sender<TokenRequest>>,
    pub(crate) library: Library,
    pub(crate) playback: Playback,
    playback_login: Option<Handle>,
    pub(crate) device_name: String,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    OpenSettings,
    ShowPlayer,
    PopupClosed(Id),
    ConfigChanged(AppConfig),
    Token(TokenUpdate),
    ClientIdInput(String),
    Connect,
    CancelConnect,
    LoginFinished(Result<User, spotify::Error>),
    Logout,
    OpenDashboard,
    CopyRedirectUri,
    CopiedReset,
    LibraryScrolled(f32),
    ApplyLibraryScroll(f32),
    SetShowTrack(bool),
    SetPanelLook(PanelLook),
    PanelResized(Id, cosmic::iced::Size),
    RealignPopup,
    Poll,
    Tick,
    PlayerLoaded(Box<Result<Option<PlayerState>, spotify::Error>>),
    SavedLoaded(String, Result<bool, spotify::Error>),
    ArtLoaded(String, Result<Artwork, String>),
    PlayPause,
    Next,
    Previous,
    ToggleShuffle,
    CycleRepeat,
    ToggleSaved,
    SeekDrag(f64),
    SeekRelease,
    SetVolume(u8),
    VolumeCommit(u64),
    ToggleMute,
    Scroll(ScrollDelta),
    CommandDone(Result<(), spotify::Error>),
    Browse(Browse),
    EnablePlayback,
    CancelPlayback,
    PlaybackLoginFinished(Result<(), String>),
    DisablePlayback,
}

/// What the user just did, kept on screen while an older poll is still in flight.
#[derive(Clone, Debug)]
struct PlaybackHold {
    at: Instant,
    playing: Option<bool>,
    shuffle: Option<bool>,
    repeat: Option<Repeat>,
    /// Track that was on screen when skip was pressed. A later poll that still
    /// names it must not replace the track Spotify already moved on from.
    left_uri: Option<String>,
    saved: Option<bool>,
}

impl PlaybackHold {
    fn touch() -> Self {
        Self {
            at: Instant::now(),
            playing: None,
            shuffle: None,
            repeat: None,
            left_uri: None,
            saved: None,
        }
    }

    fn fresh(&self) -> bool {
        self.at.elapsed() <= CONTROL_HOLD
    }

    fn is_empty(&self) -> bool {
        self.playing.is_none()
            && self.shuffle.is_none()
            && self.repeat.is_none()
            && self.left_uri.is_none()
            && self.saved.is_none()
    }

    /// Paints the chosen controls over a poll and drops each one once it matches.
    fn overlay(&mut self, player: &mut PlayerState) {
        if let Some(playing) = self.playing {
            if player.is_playing == playing {
                self.playing = None;
            } else {
                player.is_playing = playing;
            }
        }
        if let Some(shuffle) = self.shuffle {
            if player.shuffle == shuffle {
                self.shuffle = None;
            } else {
                player.shuffle = shuffle;
            }
        }
        if let Some(repeat) = self.repeat {
            if player.repeat == repeat {
                self.repeat = None;
            } else {
                player.repeat = repeat;
            }
        }
    }

    /// A poll of the track we already skipped, after the new one is on screen.
    fn stale_track(&self, incoming: Option<&str>, shown: Option<&str>) -> bool {
        match (self.left_uri.as_deref(), incoming, shown) {
            (Some(left), Some(incoming), Some(shown)) => incoming == left && shown != left,
            _ => false,
        }
    }
}

impl cosmic::Application for Window {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Action<Self::Message>>) {
        let (config_handler, config) = config::load(APP_ID);
        let spotify = Store::for_app(STATE_DIR).and_then(Spotify::new);
        let session = spotify
            .as_ref()
            .map_or(Session::SignedOut, Spotify::session);
        let error = spotify.as_ref().err().map(ToString::to_string);

        let mut window = Self {
            core,
            popup: None,
            panel_size: None,
            view: View::Player,
            config_handler,
            client_id_draft: config.client_id.clone(),
            config,
            spotify,
            session,
            login: None,
            player: None,
            fetched_at: Instant::now(),
            loaded: false,
            last_item: load_last_item(),
            error,
            saved: None,
            saved_uri: String::new(),
            artwork: None,
            art_pending: None,
            seek_drag: None,
            volume_generation: 0,
            volume_held: None,
            playback_held: None,
            progress_held: None,
            volume_before_mute: None,
            scroll_notches: 0.0,
            copied: false,
            token_tx: None,
            library: Library::default(),
            playback: Playback::Missing,
            playback_login: None,
            device_name: player::device_name(),
        };
        window.sync_playback();
        let task = window.refresh_player();
        (window, task)
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    #[allow(clippy::too_many_lines)]
    fn update(&mut self, message: Message) -> Task<Action<Self::Message>> {
        match message {
            Message::TogglePopup => {
                if self.popup.is_some() {
                    return self.close_popup();
                }
                self.view = View::Player;
                self.ensure_receiver();
                return Task::batch([
                    self.open_popup(),
                    self.refresh_player(),
                    self.ensure_library(),
                ]);
            }
            Message::OpenSettings => {
                if self.popup.is_none() {
                    self.view = View::Settings;
                    return self.open_popup();
                }
                if self.view == View::Settings {
                    return self.close_popup();
                }
                self.view = View::Settings;
            }
            Message::ShowPlayer => {
                self.view = View::Player;
                return Task::batch([self.ensure_library(), self.restore_library_scroll()]);
            }
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                    self.view = View::Player;
                }
            }
            Message::ConfigChanged(config) => {
                if config.client_id != self.config.client_id {
                    self.client_id_draft.clone_from(&config.client_id);
                }
                self.config = config;
            }
            Message::Token(update) => match update {
                TokenUpdate::Init(tx) => self.token_tx = Some(tx),
                TokenUpdate::Finished => self.token_tx = None,
                TokenUpdate::ActivationToken { token, exec } if exec == PLAYER_LOGIN => {
                    return self.authorize_playback(token);
                }
                TokenUpdate::ActivationToken { token, exec } => {
                    self.launch(&exec, token.as_deref());
                }
            },
            Message::ClientIdInput(value) => self.client_id_draft = value,
            Message::Connect => return self.connect(),
            Message::CancelConnect => {
                if let Some(handle) = self.login.take() {
                    handle.abort();
                }
            }
            Message::LoginFinished(result) => {
                self.login = None;
                match result {
                    Ok(user) => {
                        self.session = Session::Connected(user);
                        self.error = None;
                        self.view = View::Player;
                        self.sync_playback();
                        // Chains the one-time playback approval right after
                        // the account one, while the browser is still in front.
                        let playback = if self.playback == Playback::NeedsLogin {
                            self.enable_playback()
                        } else {
                            Task::none()
                        };
                        return Task::batch([
                            self.refresh_player(),
                            self.ensure_library(),
                            playback,
                        ]);
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            Message::Logout => {
                if let Ok(spotify) = &self.spotify
                    && let Err(error) = spotify.logout()
                {
                    self.error = Some(error.to_string());
                }
                self.session = Session::SignedOut;
                self.player = None;
                self.last_item = None;
                let _ = std::fs::remove_file(player::state_file(LAST_ITEM_FILE));
                self.artwork = None;
                self.saved = None;
                self.saved_uri.clear();
                self.loaded = false;
                self.library = Library::default();
                self.forget_playback();
            }
            Message::OpenDashboard => self.open_url(DASHBOARD_URL),
            Message::CopyRedirectUri => {
                self.copied = true;
                return Task::batch([
                    cosmic::iced::clipboard::write(self.config.redirect_uri()),
                    delayed(Duration::from_secs(2), Message::CopiedReset),
                ]);
            }
            Message::CopiedReset => self.copied = false,
            Message::LibraryScrolled(offset) => {
                self.remember_library_scroll(offset);
                return self.enqueue_thumbs();
            }
            Message::ApplyLibraryScroll(offset) => return self.apply_library_scroll(offset),
            Message::SetShowTrack(show) => {
                self.write_config(|config, handler| config.set_show_track(handler, show));
            }
            Message::SetPanelLook(look) => {
                self.write_config(|config, handler| config.set_panel_look(handler, look));
            }
            Message::PanelResized(id, size) => {
                if self.core.main_window_id() != Some(id) {
                    return Task::none();
                }
                let next = (px(size.width), px(size.height));
                if next.0 == 0 || next.1 == 0 || self.panel_size == Some(next) {
                    return Task::none();
                }
                self.panel_size = Some(next);
                // The panel moves the button a moment after the resize.
                return delayed(Duration::from_millis(80), Message::RealignPopup);
            }
            Message::RealignPopup => return self.realign_popup(),
            Message::Poll => return self.refresh_player(),
            Message::Tick => {
                // A track that ran out locally has changed on Spotify's side too.
                if let Some(item) = self.item()
                    && self.is_playing()
                    && item.duration_ms > 0
                    && self.progress_ms() >= item.duration_ms
                {
                    return self.refresh_player();
                }
            }
            Message::PlayerLoaded(result) => match *result {
                Ok(player) => return self.apply_player(player),
                // A poll that hiccups keeps what is on screen; the next one retries.
                Err(
                    error @ (spotify::Error::Network(_)
                    | spotify::Error::Server(_)
                    | spotify::Error::RateLimited { .. }),
                ) if self.loaded => eprintln!("player poll failed: {error}"),
                Err(error) => self.handle_api_error(&error),
            },
            Message::SavedLoaded(uri, result) => {
                if uri == self.saved_uri {
                    self.saved = self.kept_saved(result.ok());
                }
            }
            Message::ArtLoaded(url, result) => {
                if self.art_pending.as_deref() == Some(url.as_str()) {
                    self.art_pending = None;
                }
                match result {
                    Ok(artwork) if self.wanted_art() == Some(url.as_str()) => {
                        self.artwork = Some(artwork);
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("unable to load cover {url}: {error}"),
                }
            }
            Message::PlayPause => {
                let playing = self.is_playing();
                self.reanchor_progress();
                if let Some(player) = self.player.as_mut() {
                    player.is_playing = !playing;
                }
                self.playback_hold().playing = Some(!playing);
                return if playing {
                    self.command(|spotify| async move { spotify.pause().await })
                } else {
                    self.command(|spotify| async move { spotify.play().await })
                };
            }
            Message::Next => {
                self.hold_skipped_track();
                return self.command(|spotify| async move { spotify.next().await });
            }
            Message::Previous => {
                self.hold_skipped_track();
                return self.command(|spotify| async move { spotify.previous().await });
            }
            Message::ToggleShuffle => {
                let Some(player) = self.player.as_mut() else {
                    return Task::none();
                };
                player.shuffle = !player.shuffle;
                let on = player.shuffle;
                self.playback_hold().shuffle = Some(on);
                return self.command(move |spotify| async move { spotify.set_shuffle(on).await });
            }
            Message::CycleRepeat => {
                let Some(player) = self.player.as_mut() else {
                    return Task::none();
                };
                player.repeat = player.repeat.next();
                let mode = player.repeat;
                self.playback_hold().repeat = Some(mode);
                return self.command(move |spotify| async move { spotify.set_repeat(mode).await });
            }
            Message::ToggleSaved => {
                let Some(uri) = self.item().map(|item| item.uri.clone()) else {
                    return Task::none();
                };
                let save = !self.saved.unwrap_or(false);
                self.saved = Some(save);
                self.playback_hold().saved = Some(save);
                return self.command(move |spotify| async move {
                    if save {
                        spotify.save(&uri).await
                    } else {
                        spotify.unsave(&uri).await
                    }
                });
            }
            Message::SeekDrag(position) => {
                if self.item().is_some() {
                    self.seek_drag = Some(position);
                }
            }
            Message::SeekRelease => {
                let Some(position) = self.seek_drag.take() else {
                    return Task::none();
                };
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let position = position.max(0.0) as u64;
                if let Some(player) = self.player.as_mut() {
                    player.progress_ms = position;
                }
                self.fetched_at = Instant::now();
                self.hold_progress();
                return self.command(move |spotify| async move { spotify.seek(position).await });
            }
            Message::SetVolume(volume) => return self.set_volume(volume),
            Message::VolumeCommit(generation) => {
                if generation != self.volume_generation {
                    return Task::none();
                }
                let Some(volume) = self.volume() else {
                    return Task::none();
                };
                return self
                    .command(move |spotify| async move { spotify.set_volume(volume).await });
            }
            Message::ToggleMute => {
                let Some(volume) = self.volume() else {
                    return Task::none();
                };
                if volume > 0 {
                    self.volume_before_mute = Some(volume);
                    return self.set_volume(0);
                }
                let restored = self.volume_before_mute.take().unwrap_or(50);
                return self.set_volume(restored);
            }
            Message::Scroll(delta) => return self.scroll_volume(delta),
            Message::CommandDone(result) => {
                if let Err(error) = &result {
                    // The optimistic icon was a guess. Let the next poll show
                    // what Spotify actually did.
                    self.playback_held = None;
                    // Play, pause and the rest often fail while Spotify is
                    // catching up, then the next poll is fine. That used to
                    // flash the red status under the title.
                    match error {
                        spotify::Error::Reauth | spotify::Error::SignedOut => {
                            self.handle_api_error(error);
                        }
                        other => eprintln!("player command failed: {other}"),
                    }
                }
                return self.refresh_player();
            }
            Message::Browse(message) => return self.update_browse(message),
            Message::EnablePlayback => return self.enable_playback(),
            Message::CancelPlayback => {
                self.playback_login = None;
                if self.playback == Playback::Authorizing {
                    self.playback = Playback::NeedsLogin;
                }
                self.sync_playback();
            }
            Message::PlaybackLoginFinished(result) => {
                self.playback_login = None;
                self.playback = Playback::NeedsLogin;
                self.sync_playback();
                match result {
                    Ok(()) => {
                        self.error = None;
                        return self.notify(fl!("playback-ready"));
                    }
                    Err(error) if self.playback != Playback::Ready => self.error = Some(error),
                    Err(_) => {}
                }
            }
            Message::DisablePlayback => self.forget_playback(),
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            self.core.watch_config::<AppConfig>(APP_ID).map(|update| {
                for error in update.errors {
                    eprintln!("config watch error: {error}");
                }
                Message::ConfigChanged(update.config)
            }),
            activation_token_subscription(0).map(Message::Token),
            cosmic::iced::window::resize_events().map(|(id, size)| Message::PanelResized(id, size)),
        ];
        if matches!(self.session, Session::Connected(_)) {
            subscriptions.push(time::every(self.poll_interval()).map(|_| Message::Poll));
            if self.popup.is_some() && self.is_playing() {
                subscriptions.push(time::every(Duration::from_secs(1)).map(|_| Message::Tick));
            }
        }
        Subscription::batch(subscriptions)
    }

    fn view(&self) -> Element<'_, Message> {
        ui::panel(self)
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        ui::popup(self)
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl Window {
    pub(crate) fn client(&self) -> Option<Spotify> {
        self.spotify.as_ref().ok().cloned()
    }

    pub(crate) fn item(&self) -> Option<&Item> {
        self.player.as_ref().and_then(|player| player.item.as_ref())
    }

    /// What the panel shows: the current track, else the last one heard.
    pub(crate) fn shown_item(&self) -> Option<&Item> {
        self.item().or(self.last_item.as_ref())
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.player
            .as_ref()
            .is_some_and(|player| player.is_playing && player.item.is_some())
    }

    pub(crate) fn is_connecting(&self) -> bool {
        self.login.is_some()
    }

    pub(crate) fn volume(&self) -> Option<u8> {
        self.player
            .as_ref()
            .and_then(|player| player.device.as_ref())
            .filter(|device| device.supports_volume)
            .and_then(|device| device.volume)
    }

    /// Advances between polls from the last reported position.
    pub(crate) fn progress_ms(&self) -> u64 {
        if let Some(position) = self.seek_drag {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            return position.max(0.0) as u64;
        }
        if let Some((held, at, playing)) = self.progress_held {
            let mut progress = held;
            if playing {
                let elapsed = u64::try_from(at.elapsed().as_millis()).unwrap_or(0);
                progress = progress.saturating_add(elapsed);
            }
            let duration = self
                .player
                .as_ref()
                .and_then(|player| player.item.as_ref())
                .map_or(0, |item| item.duration_ms);
            return if duration > 0 {
                progress.min(duration)
            } else {
                progress
            };
        }
        let Some(player) = self.player.as_ref() else {
            return 0;
        };
        let duration = player.item.as_ref().map_or(0, |item| item.duration_ms);
        let mut progress = player.progress_ms;
        if player.is_playing {
            let elapsed = u64::try_from(self.fetched_at.elapsed().as_millis()).unwrap_or(u64::MAX);
            progress = progress.saturating_add(elapsed);
        }
        if duration > 0 {
            progress.min(duration)
        } else {
            progress
        }
    }

    fn reanchor_progress(&mut self) {
        let progress = self.progress_ms();
        if let Some(player) = self.player.as_mut() {
            player.progress_ms = progress;
        }
        self.fetched_at = Instant::now();
    }

    fn playback_hold(&mut self) -> &mut PlaybackHold {
        if self.playback_held.as_ref().is_none_or(|hold| !hold.fresh()) {
            self.playback_held = Some(PlaybackHold::touch());
        }
        let hold = self
            .playback_held
            .as_mut()
            .expect("playback hold was just inserted");
        hold.at = Instant::now();
        hold
    }

    fn hold_skipped_track(&mut self) {
        let Some(uri) = self.item().map(|item| item.uri.clone()) else {
            return;
        };
        self.playback_hold().left_uri = Some(uri);
    }

    /// Keeps the heart as toggled when an older save-check comes back.
    fn kept_saved(&mut self, reported: Option<bool>) -> Option<bool> {
        let fresh = self.playback_held.as_ref().is_some_and(PlaybackHold::fresh);
        if self.playback_held.is_some() && !fresh {
            self.playback_held = None;
            return reported;
        }
        let Some(want) = self.playback_held.as_ref().and_then(|hold| hold.saved) else {
            return reported;
        };
        if reported == Some(want) {
            if let Some(hold) = self.playback_held.as_mut() {
                hold.saved = None;
            }
            if self
                .playback_held
                .as_ref()
                .is_some_and(PlaybackHold::is_empty)
            {
                self.playback_held = None;
            }
        }
        Some(want)
    }

    /// Keeps the progress bar still while Spotify restarts the same track.
    pub(crate) fn hold_progress(&mut self) {
        let progress = self.progress_ms();
        self.progress_held = Some((progress, Instant::now(), self.is_playing()));
    }

    fn poll_interval(&self) -> Duration {
        Duration::from_secs(if self.popup.is_some() {
            4
        } else if self.is_playing() {
            15
        } else {
            60
        })
    }

    fn wanted_art(&self) -> Option<&str> {
        self.item()
            .or(self.last_item.as_ref())
            .and_then(|item| item.art_url.as_deref())
    }

    fn write_config(
        &mut self,
        set: impl FnOnce(&mut AppConfig, &Config) -> Result<bool, cosmic_config::Error>,
    ) {
        let Some(handler) = self.config_handler.as_ref() else {
            eprintln!("unable to save config: no config handler");
            return;
        };
        if let Err(error) = set(&mut self.config, handler) {
            eprintln!("unable to save config: {error}");
        }
    }

    /// Asks the compositor for an activation token first, so the browser comes
    /// to the front; the URL travels with the request and comes back with the token.
    fn open_url(&mut self, url: &str) {
        if let Some(tx) = &self.token_tx
            && tx
                .send(TokenRequest {
                    app_id: APP_ID.to_owned(),
                    exec: url.to_owned(),
                })
                .is_ok()
        {
            return;
        }
        self.launch(url, None);
    }

    fn launch(&mut self, url: &str, token: Option<&str>) {
        if let Err(error) = browser::open(url, token) {
            self.error = Some(spotify::Error::Browser(error).to_string());
        }
    }

    fn connect(&mut self) -> Task<Action<Message>> {
        if self.login.is_some() {
            return Task::none();
        }
        let spotify = match &self.spotify {
            Ok(spotify) => spotify.clone(),
            Err(error) => {
                self.error = Some(error.to_string());
                return Task::none();
            }
        };

        let client_id = self.client_id_draft.trim().to_owned();
        self.client_id_draft.clone_from(&client_id);
        if client_id != self.config.client_id {
            let value = client_id.clone();
            self.write_config(|config, handler| config.set_client_id(handler, value));
        }

        let pending = match Spotify::begin_login(&client_id, self.config.redirect_port) {
            Ok(pending) => pending,
            Err(error) => {
                self.error = Some(error.to_string());
                return Task::none();
            }
        };
        self.error = None;
        self.open_url(pending.url().as_str());

        let (task, handle) = Task::perform(
            async move { spotify.complete_login(pending).await },
            |result| Action::App(Message::LoginFinished(result)),
        )
        .abortable();
        // Aborting drops the future, which also closes the callback listener.
        self.login = Some(handle.abort_on_drop());
        task
    }

    /// Re-reads whether the receiver is installed and authorized, and points
    /// playback at it when it is.
    fn sync_playback(&mut self) {
        if self.playback == Playback::Authorizing {
            return;
        }
        self.playback = if player::binary().is_none() {
            Playback::Missing
        } else if player::authorized() {
            Playback::Ready
        } else {
            Playback::NeedsLogin
        };
        let local = (self.playback == Playback::Ready).then(|| self.device_name.clone());
        if let Ok(spotify) = &self.spotify {
            spotify.set_local_device(local);
        }
        self.ensure_receiver();
    }

    /// Restarts the receiver if it is not running, e.g. after a reboot or
    /// when its session kept dropping.
    fn ensure_receiver(&mut self) {
        if self.playback != Playback::Ready
            || !matches!(self.session, Session::Connected(_))
            || player::running()
        {
            return;
        }
        if let Err(error) = player::start() {
            self.error = Some(error);
        }
    }

    fn enable_playback(&mut self) -> Task<Action<Message>> {
        match self.playback {
            Playback::Missing => {
                self.error = Some(fl!("playback-binary-missing", binary = player::BINARY));
                Task::none()
            }
            Playback::Authorizing | Playback::Ready => Task::none(),
            Playback::NeedsLogin => {
                self.playback = Playback::Authorizing;
                self.error = None;
                if let Some(tx) = &self.token_tx
                    && tx
                        .send(TokenRequest {
                            app_id: APP_ID.to_owned(),
                            exec: PLAYER_LOGIN.to_owned(),
                        })
                        .is_ok()
                {
                    return Task::none();
                }
                self.authorize_playback(None)
            }
        }
    }

    /// Runs once the activation token is back (or could not be had).
    fn authorize_playback(&mut self, token: Option<String>) -> Task<Action<Message>> {
        // Cancelled while the token was on its way.
        if self.playback != Playback::Authorizing || self.playback_login.is_some() {
            return Task::none();
        }
        let (task, handle) = Task::perform(player::login(token), |result| {
            Action::App(Message::PlaybackLoginFinished(result))
        })
        .abortable();
        // Aborting drops the future, which kills the login process.
        self.playback_login = Some(handle.abort_on_drop());
        task
    }

    fn forget_playback(&mut self) {
        self.playback_login = None;
        player::forget();
        if let Ok(spotify) = &self.spotify {
            spotify.set_local_device(None);
        }
        self.playback = if player::binary().is_some() {
            Playback::NeedsLogin
        } else {
            Playback::Missing
        };
    }

    pub(crate) fn refresh_player(&self) -> Task<Action<Message>> {
        let Ok(spotify) = &self.spotify else {
            return Task::none();
        };
        if !matches!(self.session, Session::Connected(_)) {
            return Task::none();
        }
        let spotify = spotify.clone();
        Task::perform(async move { spotify.player().await }, |result| {
            Action::App(Message::PlayerLoaded(Box::new(result)))
        })
    }

    fn apply_player(&mut self, mut player: Option<PlayerState>) -> Task<Action<Message>> {
        // A poll that left before the volume command must not pull the slider back.
        if let Some((held, at)) = self.volume_held {
            let reported = player
                .as_ref()
                .and_then(|player| player.device.as_ref())
                .and_then(|device| device.volume);
            if reported == Some(held) || at.elapsed() > VOLUME_HOLD {
                self.volume_held = None;
            } else if let Some(device) = player.as_mut().and_then(|player| player.device.as_mut()) {
                device.volume = Some(held);
            }
        }
        let shown_uri = self.item().map(|item| item.uri.clone());
        let mut drop_hold = false;
        let mut stale_track = false;
        if let Some(hold) = self.playback_held.as_mut() {
            if !hold.fresh() {
                drop_hold = true;
            } else if let Some(incoming) = player.as_mut() {
                stale_track = hold.stale_track(
                    incoming.item.as_ref().map(|item| item.uri.as_str()),
                    shown_uri.as_deref(),
                );
                hold.overlay(incoming);
                drop_hold = hold.is_empty();
            }
        }
        if drop_hold {
            self.playback_held = None;
        }
        if stale_track {
            return Task::none();
        }
        if let Some((held, at, playing)) = self.progress_held {
            let elapsed = u64::try_from(at.elapsed().as_millis()).unwrap_or(0);
            let expected = if playing {
                held.saturating_add(elapsed)
            } else {
                held
            };
            let reported = player.as_ref().map(|player| player.progress_ms);
            let caught_up = reported.is_some_and(|ms| ms.abs_diff(expected) < 4_000);
            if caught_up || at.elapsed() > Duration::from_secs(5) {
                self.progress_held = None;
            }
        }
        // Rebuilding the queue makes Spotify answer "nothing playing" for a
        // moment. Dropping the track here shrinks the panel back to the icon.
        if self.progress_held.is_some()
            && player.as_ref().is_none_or(|player| player.item.is_none())
        {
            return Task::none();
        }
        if let Some(item) = self.item() {
            self.last_item = Some(item.clone());
        }
        self.player = player;
        self.fetched_at = Instant::now();
        self.loaded = true;
        self.error = None;
        if let Some(item) = self.item().cloned()
            && self.last_item.as_ref() != Some(&item)
        {
            save_last_item(&item);
            self.last_item = Some(item);
        }

        let mut tasks = Vec::new();
        if let Some(item) = self.item().cloned()
            && shown_uri.as_deref() != Some(item.uri.as_str())
        {
            self.library.remember_item(&item);
            tasks.push(self.note_recent_stale());
            tasks.push(self.note_queue_stale());
        }
        if let Some(uri) = self.item().map(|item| item.uri.clone())
            && uri != self.saved_uri
        {
            self.saved_uri.clone_from(&uri);
            self.saved = None;
            tasks.push(self.check_saved(uri));
        }
        if let Some(url) = self.wanted_art().map(str::to_owned)
            && self.artwork.as_ref().is_none_or(|art| art.url != url)
            && self.art_pending.as_deref() != Some(url.as_str())
        {
            self.art_pending = Some(url.clone());
            tasks.push(self.load_art(url));
        }
        Task::batch(tasks)
    }

    /// Play history only grows once a track has moved on, so mark it dirty then.
    fn note_recent_stale(&mut self) -> Task<Action<Message>> {
        self.library.mark_recent_dirty();
        let showing = self.popup.is_some()
            && self.library.detail.is_none()
            && self.library.tab == crate::browse::Tab::Search
            && self.library.query.trim().is_empty();
        if showing {
            self.ensure_library()
        } else {
            Task::none()
        }
    }

    /// A new track means a new "up next", whether it came from a playlist, the
    /// queue itself, or another device.
    fn note_queue_stale(&mut self) -> Task<Action<Message>> {
        let showing = self.popup.is_some()
            && self.library.detail.is_none()
            && self.library.tab == crate::browse::Tab::Queue;
        if showing {
            self.refresh_queue()
        } else {
            self.library.queue = crate::browse::Load::Idle;
            Task::none()
        }
    }

    fn check_saved(&self, uri: String) -> Task<Action<Message>> {
        let Ok(spotify) = &self.spotify else {
            return Task::none();
        };
        let spotify = spotify.clone();
        Task::perform(
            async move {
                let result = spotify.is_saved(&uri).await;
                (uri, result)
            },
            |(uri, result)| Action::App(Message::SavedLoaded(uri, result)),
        )
    }

    fn load_art(&self, url: String) -> Task<Action<Message>> {
        let Ok(spotify) = &self.spotify else {
            return Task::none();
        };
        let spotify = spotify.clone();
        Task::perform(
            async move {
                let result = match art::fetch(url.clone(), spotify).await {
                    Ok(bytes) => art::decode(url.clone(), &bytes),
                    Err(error) => Err(error.to_string()),
                };
                (url, result)
            },
            |(url, result)| Action::App(Message::ArtLoaded(url, result)),
        )
    }

    /// Runs a player command, then lets Spotify settle before the state is re-read.
    pub(crate) fn command<F, Fut>(&self, run: F) -> Task<Action<Message>>
    where
        F: FnOnce(Spotify) -> Fut,
        Fut: Future<Output = Result<(), spotify::Error>> + Send + 'static,
    {
        let Ok(spotify) = &self.spotify else {
            return Task::none();
        };
        let pending = run(spotify.clone());
        Task::perform(
            async move {
                let result = pending.await;
                if result.is_ok() {
                    tokio::time::sleep(SETTLE).await;
                }
                result
            },
            |result| Action::App(Message::CommandDone(result)),
        )
    }

    /// Moves the slider at once and sends only the value it settles on.
    fn set_volume(&mut self, volume: u8) -> Task<Action<Message>> {
        let Some(device) = self
            .player
            .as_mut()
            .and_then(|player| player.device.as_mut())
            .filter(|device| device.supports_volume)
        else {
            return Task::none();
        };
        let volume = volume.min(100);
        device.volume = Some(volume);
        self.volume_held = Some((volume, Instant::now()));
        self.volume_generation += 1;
        delayed(
            VOLUME_DEBOUNCE,
            Message::VolumeCommit(self.volume_generation),
        )
    }

    fn scroll_volume(&mut self, delta: ScrollDelta) -> Task<Action<Message>> {
        let Some(volume) = self.volume() else {
            return Task::none();
        };
        self.scroll_notches += match delta {
            ScrollDelta::Lines { y, .. } => y,
            ScrollDelta::Pixels { y, .. } => y / PIXELS_PER_NOTCH,
        };
        #[allow(clippy::cast_possible_truncation)]
        let notches = self.scroll_notches.trunc() as i16;
        if notches == 0 {
            return Task::none();
        }
        self.scroll_notches -= f32::from(notches);
        let target = (i16::from(volume) + notches * VOLUME_STEP).clamp(0, 100);
        self.set_volume(u8::try_from(target).unwrap_or(0))
    }

    pub(crate) fn handle_api_error(&mut self, error: &spotify::Error) {
        match error {
            spotify::Error::Reauth => self.session = Session::NeedsReauth,
            spotify::Error::SignedOut => self.session = Session::SignedOut,
            _ => {}
        }
        self.error = Some(error.to_string());
    }

    fn close_popup(&mut self) -> Task<Action<Message>> {
        let Some(popup) = self.popup.take() else {
            return Task::none();
        };
        self.view = View::Player;
        self.library.devices = None;
        surface_task(destroy_popup(popup))
    }

    /// Anchors the popup to the panel button's real rectangle. The default
    /// one is only icon-sized, so it drifts once the button grows or shrinks.
    fn popup_settings(
        &self,
        parent: Id,
        popup: Id,
    ) -> cosmic::iced::platform_specific::runtime::wayland::popup::SctkPopupSettings {
        let mut settings = self
            .core
            .applet
            .get_popup_settings(parent, popup, None, None, None);
        settings.positioner.size_limits = Limits::NONE
            .min_width(360.0)
            .max_width(ui::POPUP_WIDTH)
            .min_height(120.0)
            .max_height(1000.0);
        match self.core.applet.anchor {
            PanelAnchor::Top => {
                settings.positioner.anchor = Anchor::BottomLeft;
                settings.positioner.gravity = Gravity::BottomRight;
            }
            PanelAnchor::Bottom => {
                settings.positioner.anchor = Anchor::TopLeft;
                settings.positioner.gravity = Gravity::TopRight;
            }
            PanelAnchor::Left | PanelAnchor::Right => {}
        }
        if let Some((width, height)) = self.panel_size {
            settings.positioner.anchor_rect.width = i32::try_from(width).unwrap_or(i32::MAX);
            settings.positioner.anchor_rect.height = i32::try_from(height).unwrap_or(i32::MAX);
        }
        settings
    }

    fn realign_popup(&self) -> Task<Action<Message>> {
        let (Some(popup), Some(parent)) = (self.popup, self.core.main_window_id()) else {
            return Task::none();
        };
        let settings = self.popup_settings(parent, popup);
        cosmic::iced::platform_specific::shell::wayland::commands::popup::reposition(
            popup,
            settings.positioner,
        )
    }

    fn open_popup(&mut self) -> Task<Action<Message>> {
        let Some(parent) = self.core.main_window_id() else {
            return Task::none();
        };

        let open = surface_task(app_popup::<Window>(
            |_| LiveSettings::default(),
            move |state: &mut Window| {
                let popup = Id::unique();
                let settings = state.popup_settings(parent, popup);
                state.popup = Some(popup);
                settings
            },
            Some(Box::new(|state: &Window| {
                ui::popup(state).map(cosmic::Action::App)
            })),
        ));
        if self.view == View::Player {
            Task::batch([open, self.restore_library_scroll()])
        } else {
            open
        }
    }
}

const LAST_ITEM_FILE: &str = "last-item.json";

/// The track the panel showed before a restart, so it does not come up empty.
fn load_last_item() -> Option<Item> {
    let bytes = std::fs::read(player::state_file(LAST_ITEM_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_last_item(item: &Item) {
    let path = player::state_file(LAST_ITEM_FILE);
    let Ok(bytes) = serde_json::to_vec(item) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(error) = std::fs::write(&path, bytes) {
        eprintln!("unable to remember the last track: {error}");
    }
}

pub(crate) fn delayed(after: Duration, message: Message) -> Task<Action<Message>> {
    Task::perform(tokio::time::sleep(after), move |()| {
        Action::App(message.clone())
    })
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn px(value: f32) -> u32 {
    value.round().max(0.0) as u32
}

fn surface_task(action: cosmic::surface::Action<Message>) -> Task<Action<Message>> {
    cosmic::task::message(cosmic::Action::Surface(action))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(playing: bool) -> PlayerState {
        PlayerState {
            is_playing: playing,
            progress_ms: 0,
            shuffle: false,
            repeat: Repeat::Off,
            device: None,
            item: None,
        }
    }

    #[test]
    fn a_stale_poll_does_not_undo_pause() {
        let mut hold = PlaybackHold::touch();
        hold.playing = Some(false);
        let mut incoming = player(true);
        hold.overlay(&mut incoming);
        assert!(!incoming.is_playing);
        assert_eq!(hold.playing, Some(false));

        incoming.is_playing = false;
        hold.overlay(&mut incoming);
        assert!(hold.playing.is_none());
    }

    #[test]
    fn a_skipped_track_does_not_come_back() {
        let mut hold = PlaybackHold::touch();
        hold.left_uri = Some("spotify:track:a".into());
        assert!(hold.stale_track(Some("spotify:track:a"), Some("spotify:track:b")));
        assert!(!hold.stale_track(Some("spotify:track:b"), Some("spotify:track:b")));
        assert!(!hold.stale_track(Some("spotify:track:a"), Some("spotify:track:a")));
    }
}
