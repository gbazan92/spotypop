use cosmic::Element;
use cosmic::iced::advanced::text::{Ellipsize, EllipsizeHeightLimit, LineHeight, Wrapping};
use cosmic::iced::widget::scrollable::{Direction, Scrollbar};
use cosmic::iced::{Alignment, Color, Length, Limits};
use cosmic::theme;
use cosmic::widget::button::Catalog;
use cosmic::widget::segmented_button::StyleSheet;
use cosmic::widget::{
    Column, Id, Row, button, container, divider, dropdown, icon, image, mouse_area, scrollable,
    segmented_control, settings, slider, space, text, text_input, toggler, tooltip,
};

use crate::browse::{Browse, Load, Source, Tab};
use crate::config::PanelLook;
use crate::fl;
use crate::look::{self, scope};
use crate::marquee::{marquee, marquee_fill};
use crate::spotify::{Device, Entry, EntryKind, Item, ItemKind, Repeat, Session};
use crate::window::{Message, Playback, View, Window};

pub const POPUP_WIDTH: f32 = 400.0;
/// Width of what the bar shows next to the logo while a track is up, whatever
/// the look: the scopes fill it, and the cover plus its title add up to it.
/// Longer titles scroll instead of widening the panel.
const PANEL_CHIP_WIDTH: f32 = 180.0;
const CHIP_SPACING: f32 = 8.0;
pub const HERO_SIZE: f32 = 96.0;
const HERO_RADIUS: f32 = 10.0;
const VOLUME_SLIDER_WIDTH: f32 = 110.0;
const ROW_ART: f32 = 40.0;
const LIST_HEIGHT: f32 = 300.0;
pub const LIBRARY_SCROLL: &str = "library-list";

const LOGO: &[u8] =
    include_bytes!("../res/icons/hicolor/scalable/apps/io.github.gbazan92.SpotyPop-symbolic.svg");

fn logo(size: u16) -> icon::Icon {
    icon::icon(icon::from_svg_bytes(LOGO).symbolic(true)).size(size)
}

fn one_line(
    widget: cosmic::widget::Text<'_, cosmic::Theme>,
) -> cosmic::widget::Text<'_, cosmic::Theme> {
    widget
        .wrapping(Wrapping::None)
        .ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1)))
}

fn dim_style(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    let mut color = Color::from(theme.cosmic().on_bg_color());
    color.a *= 0.65;
    cosmic::iced::widget::text::Style {
        color: Some(color),
        ..Default::default()
    }
}

fn error_style(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    cosmic::iced::widget::text::Style {
        color: Some(theme.cosmic().destructive_text_color().into()),
        ..Default::default()
    }
}

const DIM: theme::Text = theme::Text::Custom(dim_style);
const ERROR: theme::Text = theme::Text::Custom(error_style);

pub fn format_time(ms: u64) -> String {
    let seconds = ms / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn subtitle(item: &Item) -> String {
    match item.kind {
        ItemKind::Track if !item.album.is_empty() && !item.subtitle.is_empty() => {
            format!("{}  ·  {}", item.subtitle, item.album)
        }
        _ => item.subtitle.clone(),
    }
}

fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "•".repeat(chars.len());
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

// ---------------------------------------------------------------- panel

fn button_padding(state: &Window) -> (u16, u16) {
    let applet = &state.core.applet;
    let (major, minor) = applet.suggested_padding(true);
    if applet.is_horizontal() {
        (major, minor)
    } else {
        (minor, major)
    }
}

/// Spotify drops the item after a pause or a hiccup. The bar keeps the last
/// one so it does not collapse to the cover and grow back a moment later.
fn shows_chip(state: &Window) -> bool {
    state.core.applet.is_horizontal()
        && matches!(state.session, Session::Connected(_))
        && state.config.show_track
        && state.shown_item().is_some()
}

/// The size [`panel`] gives the button. The panel does not report the
/// applet's size when it follows its content, so the popup is anchored to this.
pub fn panel_button_size(state: &Window) -> (f32, f32) {
    let (icon_size, _) = state.core.applet.suggested_size(true);
    let (pad_x, pad_y) = button_padding(state);
    let icon = f32::from(icon_size);
    let content = if shows_chip(state) {
        PANEL_CHIP_WIDTH
    } else {
        icon
    };
    (
        content + 2.0 * f32::from(pad_x),
        icon + 2.0 * f32::from(pad_y),
    )
}

pub fn panel(state: &Window) -> Element<'_, Message> {
    let applet = &state.core.applet;
    let (icon_size, _) = applet.suggested_size(true);
    let (pad_x, pad_y) = button_padding(state);

    let connected = matches!(state.session, Session::Connected(_));
    // Same box as the logo, so switching between them never resizes the panel.
    let cover_size = f32::from(icon_size);
    let cover: Element<'_, Message> = match state
        .artwork
        .as_ref()
        .filter(|_| connected && state.config.show_track)
    {
        Some(artwork) => {
            let opacity: f32 = if state.is_playing() {
                1.0
            } else if state.item().is_some() {
                0.72
            } else {
                0.5
            };
            image(artwork.thumb.clone())
                .width(cover_size)
                .height(cover_size)
                .border_radius(cover_size * 0.22)
                .opacity(opacity)
                .into()
        }
        None => logo(icon_size).into(),
    };

    let content = if shows_chip(state) {
        playing_chip(state, cover_size, cover)
    } else {
        cover
    };

    let content = container(content)
        .height(cover_size)
        .align_y(Alignment::Center)
        .clip(true);

    let button = button::custom(content)
        .padding([pad_y, pad_x])
        .class(theme::Button::AppletIcon)
        .on_press(Message::TogglePopup);

    let area = mouse_area(button).on_right_press(Message::OpenSettings);

    applet.autosize_window(area).into()
}

/// Title line for the bar: song, then the artist after a dot.
fn track_label(state: &Window) -> String {
    let Some(item) = state.shown_item() else {
        return String::new();
    };
    if item.subtitle.is_empty() {
        item.name.clone()
    } else {
        format!("{}  ·  {}", item.name, item.subtitle)
    }
}

fn scrolling_label(state: &Window, cover_size: f32) -> Element<'_, Message> {
    let applet = &state.core.applet;
    let width = (PANEL_CHIP_WIDTH - cover_size - CHIP_SPACING).max(0.0);
    // The preset line height is taller than the icon, which stretches the bar.
    container(marquee(
        applet
            .text(track_label(state))
            .wrapping(Wrapping::None)
            .line_height(LineHeight::Absolute(cover_size.into())),
        width,
    ))
    .width(Length::Fixed(width))
    .into()
}

fn playing_chip<'a>(
    state: &'a Window,
    cover_size: f32,
    cover: Element<'a, Message>,
) -> Element<'a, Message> {
    let kind = match state.config.panel_look {
        PanelLook::Cover => {
            return Row::new()
                .spacing(CHIP_SPACING)
                .align_y(Alignment::Center)
                .push(cover)
                .push(scrolling_label(state, cover_size))
                .into();
        }
        PanelLook::Bars => look::ScopeKind::Bars,
        PanelLook::Wave => look::ScopeKind::Wave,
        PanelLook::Fill => look::ScopeKind::Fill,
    };
    scope(kind, PANEL_CHIP_WIDTH, cover_size, state.is_playing())
}

// ---------------------------------------------------------------- popup

pub fn popup(state: &Window) -> Element<'_, Message> {
    let content = match (state.view, &state.session) {
        (View::Settings, _) => settings_view(state),
        (View::Player, Session::Connected(_)) => player_view(state),
        (View::Player, _) => setup_view(state),
    };
    state
        .core
        .applet
        .popup_container(container(content).padding(16).width(Length::Fill))
        .limits(
            Limits::NONE
                .min_height(1.0)
                .min_width(POPUP_WIDTH)
                .max_width(POPUP_WIDTH)
                .max_height(1000.0),
        )
        .into()
}

fn header<'a>(
    status: Element<'a, Message>,
    actions: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut row = Row::new()
        .spacing(10)
        .align_y(Alignment::Center)
        .push(logo(24))
        .push(
            Column::new()
                .spacing(2)
                .width(Length::Fill)
                .push(text::heading("SpotyPop"))
                .push(status),
        );
    if let Some(actions) = actions {
        row = row.push(actions);
    }
    row.into()
}

fn status_line(state: &Window, fallback: String) -> Element<'_, Message> {
    match &state.error {
        Some(error) => text::caption(error.as_str()).class(ERROR).into(),
        None => one_line(text::caption(fallback)).class(DIM).into(),
    }
}

// ---------------------------------------------------------------- player

fn player_view(state: &Window) -> Element<'_, Message> {
    let device = state
        .player
        .as_ref()
        .and_then(|player| player.device.as_ref());
    let active = state.item().is_some();

    let status = match (active, device) {
        _ if state.offline => fl!("offline"),
        (true, Some(device)) if state.is_playing() => {
            fl!("status-playing-on", device = device.name.as_str())
        }
        (true, Some(device)) => fl!("status-paused-on", device = device.name.as_str()),
        (true, None) => fl!("status-paused"),
        (false, _) if !state.loaded => fl!("status-loading"),
        (false, _) if state.receiver_reconnecting() => fl!("receiver-reconnecting"),
        (false, _) => fl!("nothing-playing"),
    };

    let status: Element<'_, Message> = match &state.library.notice {
        Some(notice) => one_line(text::caption(notice.as_str()))
            .class(theme::Text::Accent)
            .into(),
        None => status_line(state, status),
    };

    let device_label = device.map_or_else(|| fl!("devices"), |device| device.name.clone());
    let device_icon = device.map_or("audio-speakers-symbolic", |device| {
        device_icon(&device.kind)
    });
    let chip = button::custom(
        Row::new()
            .spacing(6)
            .align_y(Alignment::Center)
            .push(icon::from_name(device_icon).size(14))
            .push(one_line(text::caption(device_label)))
            .push(icon::from_name("pan-down-symbolic").size(12)),
    )
    .padding([4, 10])
    .class(theme::Button::Standard)
    .selected(state.library.devices.is_some())
    .on_press(Message::Browse(Browse::ToggleDevices));

    let actions = Row::new()
        .spacing(4)
        .align_y(Alignment::Center)
        .push(container(chip).max_width(160.0))
        .push(
            button::icon(icon::from_name("view-refresh-symbolic"))
                .tooltip(fl!("refresh"))
                .on_press(Message::Browse(Browse::Reload)),
        );

    let mut column = Column::new()
        .spacing(14)
        .push(header(status, Some(actions.into())))
        .push(divider::horizontal::default());
    if let Some(banner) = playback_banner(state) {
        column = column.push(banner);
    }
    if let Some(devices) = &state.library.devices {
        column = column.push(device_picker(state, devices));
    }
    column
        .push(hero(state))
        .push(transport(state))
        .push(divider::horizontal::default())
        .push(library(state))
        .into()
}

/// Offers to turn this computer into a player until it is one.
fn playback_banner(state: &Window) -> Option<Element<'_, Message>> {
    let (lead, action): (String, Option<Element<'_, Message>>) = match state.playback {
        Playback::Ready => return None,
        Playback::NeedsLogin => (
            fl!("playback-lead"),
            Some(nowrap_button(
                fl!("playback-link"),
                theme::Button::Suggested,
                Message::EnablePlayback,
            )),
        ),
        Playback::Authorizing => (
            fl!("waiting-browser-approval"),
            Some(nowrap_button(
                fl!("cancel"),
                theme::Button::Standard,
                Message::CancelPlayback,
            )),
        ),
        Playback::Missing => (fl!("playback-missing"), None),
    };
    let mut row = Row::new()
        .spacing(12)
        .align_y(Alignment::Center)
        .push(icon::from_name("computer-symbolic").size(20))
        .push(
            Column::new()
                .spacing(2)
                .width(Length::Fill)
                .push(text::heading(fl!("playback-title")))
                .push(text::caption(lead).class(DIM)),
        );
    if let Some(action) = action {
        row = row.push(action);
    }
    Some(
        container(row)
            .padding([10, 12])
            .width(Length::Fill)
            .class(theme::Container::Card)
            .into(),
    )
}

fn device_icon(kind: &str) -> &'static str {
    match kind.to_ascii_lowercase().as_str() {
        "computer" => "computer-symbolic",
        "smartphone" | "tablet" => "phone-symbolic",
        "tv" | "castvideo" => "tv-symbolic",
        _ => "audio-speakers-symbolic",
    }
}

fn device_picker<'a>(state: &'a Window, devices: &'a Load<Vec<Device>>) -> Element<'a, Message> {
    let body: Element<'a, Message> = match devices {
        Load::Idle | Load::Loading => placeholder_text(fl!("devices-searching")),
        Load::Failed(error) => text::caption(error.as_str()).class(ERROR).into(),
        Load::Ready(list) if list.is_empty() => {
            placeholder_text(if state.receiver_reconnecting() {
                fl!("receiver-reconnecting")
            } else if state.playback == Playback::Ready {
                fl!("devices-local-starting")
            } else {
                fl!("devices-none")
            })
        }
        Load::Ready(list) => {
            let active_id = state
                .player
                .as_ref()
                .and_then(|player| player.device.as_ref())
                .and_then(|device| device.id.as_deref());
            let mut column = Column::new().spacing(2);
            for device in list {
                let active = device.is_active || device.id.as_deref() == active_id;
                let mut row = Row::new()
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .push(icon::from_name(device_icon(&device.kind)).size(16))
                    .push(one_line(text::body(device.name.as_str())).width(Length::Fill));
                if active {
                    row = row.push(icon::from_name("object-select-symbolic").size(16));
                }
                let target = device
                    .id
                    .clone()
                    .filter(|_| !active && !device.is_restricted);
                column = column.push(
                    button::custom(row)
                        .padding([8, 12])
                        .width(Length::Fill)
                        .class(theme::Button::AppletMenu)
                        .selected(active)
                        .on_press_maybe(target.map(|id| Message::Browse(Browse::Transfer(id)))),
                );
            }
            column.into()
        }
    };
    container(body)
        .padding(4)
        .width(Length::Fill)
        .class(theme::Container::Card)
        .into()
}

fn cover(state: &Window) -> Element<'_, Message> {
    let dimmed = state.item().is_none();
    let art: Element<'_, Message> = match &state.artwork {
        Some(artwork) => image(artwork.hero.clone())
            .width(HERO_SIZE)
            .height(HERO_SIZE)
            .border_radius(HERO_RADIUS)
            .opacity(if dimmed { 0.55_f32 } else { 1.0 })
            .into(),
        None => container(logo(40))
            .center(HERO_SIZE)
            .class(theme::Container::Card)
            .into(),
    };
    mouse_area(art).on_press(Message::PlayPause).into()
}

fn hero(state: &Window) -> Element<'_, Message> {
    let active = state.item().is_some();
    let shown = state.item().or(state.last_item.as_ref());

    let title = match shown {
        Some(item) if active => item.name.clone(),
        _ => fl!("nothing-playing"),
    };
    let detail = match shown {
        Some(item) if active => subtitle(item),
        Some(item) => fl!("hero-last", track = item.name.as_str()),
        None if state.playback == Playback::Ready => fl!("hero-pick-below"),
        None => fl!("hero-start-elsewhere"),
    };

    let mut title_row = Row::new()
        .width(Length::Fill)
        .spacing(6)
        .align_y(Alignment::Center)
        .push(marquee_fill(text::title4(title).wrapping(Wrapping::None)));
    if active {
        title_row = title_row.push(save_button(state.saved.unwrap_or(false)));
    }

    let duration = state.item().map_or(0, |item| item.duration_ms);
    let progress = state.progress_ms();
    #[allow(clippy::cast_precision_loss)]
    let seek = slider(
        0.0..=duration.max(1) as f64,
        progress as f64,
        Message::SeekDrag,
    )
    .on_release(Message::SeekRelease)
    .step(1000.0)
    .width(Length::Fill);

    let times = Row::new()
        .push(text::caption(format_time(progress)).class(DIM))
        .push(space::horizontal())
        .push(
            text::caption(if duration > 0 {
                format_time(duration)
            } else {
                "–:––".to_owned()
            })
            .class(DIM),
        );

    let info = Column::new()
        .spacing(4)
        .width(Length::Fill)
        .push(title_row)
        .push(marquee_fill(
            text::body(detail).wrapping(Wrapping::None).class(DIM),
        ))
        .push(space::vertical().height(4))
        .push(seek)
        .push(times);

    Row::new()
        .spacing(14)
        .align_y(Alignment::Center)
        .push(cover(state))
        .push(info)
        .into()
}

fn transport(state: &Window) -> Element<'_, Message> {
    let player = state.player.as_ref();
    let active = state.item().is_some() && !state.offline;
    let shuffle = player.is_some_and(|player| player.shuffle);
    let repeat = player.map_or(Repeat::Off, |player| player.repeat);

    let small = |name: &'static str, tooltip: String, message: Message| {
        button::icon(icon::from_name(name))
            .tooltip(tooltip)
            .on_press_maybe(active.then_some(message))
    };
    // The icon style drops `selected`, so a mode that is on paints itself in the accent.
    let mode = |name: &'static str, tip: String, message: Message, on: bool| {
        let button = button::custom(icon::from_name(name).size(16))
            .padding(8)
            .class(toggle_class(on))
            .on_press_maybe(active.then_some(message));
        Element::from(tooltip(button, text::body(tip), tooltip::Position::Top))
    };

    let play_icon = if state.is_playing() {
        "media-playback-pause-symbolic"
    } else {
        "media-playback-start-symbolic"
    };
    let play = button::custom(icon::from_name(play_icon).size(20))
        .padding([6, 16])
        .class(theme::Button::Standard)
        .on_press_maybe((!state.offline).then_some(Message::PlayPause));

    let (repeat_icon, repeat_tip) = match repeat {
        Repeat::Off => ("media-playlist-consecutive-symbolic", fl!("repeat-off")),
        Repeat::Context => ("media-playlist-repeat-symbolic", fl!("repeat-all")),
        Repeat::Track => ("media-playlist-repeat-song-symbolic", fl!("repeat-track")),
    };
    let shuffle_tip = if shuffle {
        fl!("shuffle-on")
    } else {
        fl!("shuffle-off")
    };

    let volume = state.volume();
    let volume_icon = match volume {
        None | Some(0) => "audio-volume-muted-symbolic",
        Some(1..=33) => "audio-volume-low-symbolic",
        Some(34..=66) => "audio-volume-medium-symbolic",
        Some(_) => "audio-volume-high-symbolic",
    };
    let volume_controls = Row::new()
        .spacing(6)
        .align_y(Alignment::Center)
        .push(
            button::icon(icon::from_name(volume_icon))
                .tooltip(fl!("mute"))
                .on_press_maybe(volume.map(|_| Message::ToggleMute)),
        )
        .push(
            slider(0..=100u8, volume.unwrap_or(0), Message::SetVolume).width(VOLUME_SLIDER_WIDTH),
        );

    Row::new()
        .spacing(2)
        .align_y(Alignment::Center)
        .push(mode(
            "media-playlist-shuffle-symbolic",
            shuffle_tip,
            Message::ToggleShuffle,
            shuffle,
        ))
        .push(small(
            "media-skip-backward-symbolic",
            fl!("previous"),
            Message::Previous,
        ))
        .push(play)
        .push(small(
            "media-skip-forward-symbolic",
            fl!("next"),
            Message::Next,
        ))
        .push(mode(
            repeat_icon,
            repeat_tip,
            Message::CycleRepeat,
            repeat != Repeat::Off,
        ))
        .push(space::horizontal())
        .push(volume_controls)
        .into()
}

// ---------------------------------------------------------------- library

/// The icon button style drops the accent, so the heart is drawn on its own.
fn save_button(saved: bool) -> Element<'static, Message> {
    let button = button::custom(icon::from_name("emblem-favorite-symbolic").size(16))
        .padding(4)
        .class(toggle_class(saved))
        .on_press(Message::ToggleSaved);
    tooltip(
        button,
        text::body(if saved {
            fl!("library-remove")
        } else {
            fl!("library-save")
        }),
        tooltip::Position::Top,
    )
    .into()
}

/// An icon button tinted with the system accent while `on`.
fn toggle_class(on: bool) -> theme::Button {
    theme::Button::Custom {
        active: Box::new(move |focused, theme| toggle_look(theme, focused, on, Press::Idle)),
        hovered: Box::new(move |focused, theme| toggle_look(theme, focused, on, Press::Hover)),
        pressed: Box::new(move |focused, theme| toggle_look(theme, focused, on, Press::Down)),
        disabled: Box::new(|theme| toggle_look(theme, false, false, Press::Off)),
    }
}

#[derive(Clone, Copy)]
enum Press {
    Idle,
    Hover,
    Down,
    Off,
}

fn toggle_look(theme: &cosmic::Theme, focused: bool, on: bool, press: Press) -> button::Style {
    let mut style = match press {
        Press::Idle => theme.active(focused, false, &theme::Button::Icon),
        Press::Hover => theme.hovered(focused, false, &theme::Button::Icon),
        Press::Down => theme.pressed(focused, false, &theme::Button::Icon),
        Press::Off => theme.disabled(&theme::Button::Icon),
    };
    if on {
        style.icon_color = Some(Color::from(theme.cosmic().accent_color()));
    }
    style
}

fn placeholder_text<'a>(message: String) -> Element<'a, Message> {
    container(text::body(message).class(DIM).align_x(Alignment::Center))
        .padding([24, 16])
        .center_x(Length::Fill)
        .into()
}

fn library(state: &Window) -> Element<'_, Message> {
    let library = &state.library;
    let mut column = Column::new().spacing(10);

    if let Some(detail) = &library.detail {
        let (title, playable) = match &detail.source {
            Source::Liked => (fl!("liked-songs"), matches!(detail.items, Load::Ready(_))),
            Source::Entry(entry) => (entry.name.clone(), true),
        };
        column = column.push(
            Row::new()
                .spacing(6)
                .align_y(Alignment::Center)
                .push(
                    button::icon(icon::from_name("go-previous-symbolic"))
                        .tooltip(fl!("back"))
                        .on_press(Message::Browse(Browse::CloseDetail)),
                )
                .push(one_line(text::heading(title)).width(Length::Fill))
                .push(
                    button::suggested(fl!("play"))
                        .leading_icon(icon::from_name("media-playback-start-symbolic"))
                        .on_press_maybe(playable.then_some(Message::Browse(Browse::PlayDetail))),
                ),
        );
    } else {
        column = column.push(
            segmented_control::horizontal(&library.tabs)
                // Control draws a check on the active tab. The accent already marks it.
                .style(theme::SegmentedButton::Custom(Box::new(|theme| {
                    <cosmic::Theme as StyleSheet>::horizontal(
                        theme,
                        &theme::SegmentedButton::Control,
                    )
                })))
                .on_activate(|entity| Message::Browse(Browse::SelectTab(entity))),
        );
        if library.tab == Tab::Search {
            column = column.push(
                text_input::search_input(fl!("search-placeholder"), library.query.as_str())
                    .on_input(|query| Message::Browse(Browse::Query(query)))
                    .on_submit(|_| Message::Browse(Browse::SubmitSearch))
                    .on_clear(Message::Browse(Browse::ClearSearch)),
            );
        }
    }

    column
        .push(
            scrollable(list_body(state))
                .id(Id::new(LIBRARY_SCROLL))
                .on_scroll(|viewport| Message::LibraryScrolled(viewport.absolute_offset().y))
                .direction(Direction::Vertical(
                    Scrollbar::new()
                        .width(4.0)
                        .scroller_width(4.0)
                        .padding(2.0)
                        // Sit the bar beside the rows, instead of on top of the add buttons.
                        .spacing(10.0),
                ))
                .height(LIST_HEIGHT),
        )
        .into()
}

fn list_body(state: &Window) -> Element<'_, Message> {
    let library = &state.library;
    if let Some(detail) = &library.detail {
        return entries_or(state, &detail.items, fl!("empty-list"), None);
    }
    match library.tab {
        Tab::Search if library.query.trim().is_empty() => entries_or(
            state,
            &library.recent,
            fl!("empty-recent"),
            Some(section_title(fl!("recently-played"))),
        ),
        Tab::Search => match &library.results {
            Load::Idle | Load::Loading => placeholder_text(fl!("searching")),
            Load::Failed(error) => placeholder_error(error),
            Load::Ready(groups) if groups.is_empty() => placeholder_text(fl!("no-results")),
            Load::Ready(groups) => {
                let mut column = Column::new().spacing(2);
                for group in groups {
                    column = column.push(section_title(group_title(group.kind)));
                    for entry in &group.entries {
                        column = column.push(entry_row(state, entry));
                    }
                }
                column.into()
            }
        },
        Tab::Playlists => entries_or(
            state,
            &library.playlists,
            fl!("empty-playlists"),
            Some(liked_row(library.liked_count)),
        ),
        Tab::Podcasts => entries_or(state, &library.shows, fl!("empty-podcasts"), None),
        Tab::Books => entries_or(state, &library.books, fl!("empty-books"), None),
    }
}

fn entries_or<'a>(
    state: &'a Window,
    load: &'a Load<Vec<Entry>>,
    empty: String,
    header: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    match load {
        Load::Idle | Load::Loading => placeholder_text(fl!("status-loading")),
        Load::Failed(error) => placeholder_error(error),
        Load::Ready(list) => {
            let mut column = Column::new().spacing(2);
            if let Some(header) = header {
                column = column.push(header);
            }
            if list.is_empty() {
                return column.push(placeholder_text(empty)).into();
            }
            for entry in list {
                column = column.push(entry_row(state, entry));
            }
            column.into()
        }
    }
}

fn placeholder_error(error: &str) -> Element<'_, Message> {
    container(text::body(error).class(ERROR).align_x(Alignment::Center))
        .padding([24, 16])
        .center_x(Length::Fill)
        .into()
}

fn section_title<'a>(title: String) -> Element<'a, Message> {
    container(text::caption_heading(title).class(DIM))
        .padding([8, 8, 4, 8])
        .into()
}

fn group_title(kind: EntryKind) -> String {
    match kind {
        EntryKind::Track => fl!("group-tracks"),
        EntryKind::Artist => fl!("group-artists"),
        EntryKind::Album => fl!("group-albums"),
        EntryKind::Playlist => fl!("group-playlists"),
        EntryKind::Show => fl!("group-podcasts"),
        EntryKind::Episode => fl!("group-episodes"),
        EntryKind::Chapter => fl!("group-chapters"),
        EntryKind::Audiobook => fl!("group-audiobooks"),
    }
}

fn kind_icon(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Track => "audio-x-generic-symbolic",
        EntryKind::Episode | EntryKind::Show => "audio-input-microphone-symbolic",
        EntryKind::Chapter | EntryKind::Audiobook => "accessories-dictionary-symbolic",
        EntryKind::Album => "media-optical-symbolic",
        EntryKind::Artist => "avatar-default-symbolic",
        EntryKind::Playlist => "playlist-symbolic",
    }
}

fn thumb_placeholder<'a>(icon_name: &'static str, round: bool) -> Element<'a, Message> {
    container(icon::from_name(icon_name).size(18))
        .center(ROW_ART)
        .class(if round {
            theme::Container::custom(|theme| round_card(theme, ROW_ART / 2.0))
        } else {
            theme::Container::Card
        })
        .into()
}

fn round_card(theme: &cosmic::Theme, radius: f32) -> cosmic::iced::widget::container::Style {
    let component = &theme.cosmic().background(theme.transparent).component;
    cosmic::iced::widget::container::Style {
        icon_color: Some(component.on.into()),
        text_color: Some(component.on.into()),
        background: Some(Color::from(component.base).into()),
        border: cosmic::iced::Border {
            radius: radius.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn entry_row<'a>(state: &'a Window, entry: &'a Entry) -> Element<'a, Message> {
    let round = entry.kind == EntryKind::Artist;
    let thumb: Element<'a, Message> = match entry
        .art_url
        .as_ref()
        .and_then(|url| state.library.thumbs.get(url))
    {
        Some(Some(handle)) => image(handle.clone())
            .width(ROW_ART)
            .height(ROW_ART)
            .border_radius(if round { ROW_ART / 2.0 } else { 6.0 })
            .into(),
        _ => thumb_placeholder(kind_icon(entry.kind), round),
    };

    let current = state.item().is_some_and(|item| item.uri == entry.uri);
    let name = one_line(text::body(entry.name.as_str()));
    let name = if current {
        name.class(theme::Text::Accent)
    } else if entry.playable {
        name
    } else {
        name.class(DIM)
    };

    let mut row = Row::new()
        .spacing(10)
        .align_y(Alignment::Center)
        .push(thumb)
        .push(
            Column::new()
                .spacing(1)
                .width(Length::Fill)
                .push(name)
                .push(one_line(text::caption(entry.detail.as_str())).class(DIM)),
        );

    if entry.duration_ms > 0 && entry.kind.is_item() {
        row = row.push(text::caption(format_time(entry.duration_ms)).class(DIM));
    }

    let open = button::custom(row)
        .padding([6, 8])
        .width(Length::Fill)
        .class(theme::Button::AppletMenu)
        .selected(current)
        .on_press_maybe(
            entry
                .playable
                .then(|| Message::Browse(Browse::Activate(entry.clone()))),
        );

    // The action sits beside the row. Inside it, the click never arrived.
    let action = if entry.kind.is_item() {
        let queued = state.library.queued.contains(&entry.uri);
        button::icon(icon::from_name(if queued {
            "emblem-ok-symbolic"
        } else {
            "list-add-symbolic"
        }))
        .extra_small()
        .tooltip(if queued {
            fl!("queue-added")
        } else {
            fl!("queue-add")
        })
        .on_press(Message::Browse(Browse::Queue(entry.clone())))
    } else {
        button::icon(icon::from_name("media-playback-start-symbolic"))
            .extra_small()
            .tooltip(fl!("play"))
            .on_press(Message::Browse(Browse::PlayContext(entry.clone())))
    };

    Row::new()
        .spacing(4)
        .align_y(Alignment::Center)
        .push(open)
        .push(action)
        .into()
}

fn liked_row<'a>(count: u64) -> Element<'a, Message> {
    let detail = if count > 0 {
        fl!("liked-detail", count = count)
    } else {
        fl!("playlist")
    };
    let art = container(icon::from_name("emblem-favorite-symbolic").size(18))
        .center(ROW_ART)
        .class(theme::Container::custom(|theme| {
            let accent = theme.cosmic().accent_color();
            cosmic::iced::widget::container::Style {
                icon_color: Some(theme.cosmic().on_accent_color().into()),
                background: Some(Color::from(accent).into()),
                border: cosmic::iced::Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }));
    let row = Row::new()
        .spacing(10)
        .align_y(Alignment::Center)
        .push(art)
        .push(
            Column::new()
                .spacing(1)
                .width(Length::Fill)
                .push(one_line(text::body(fl!("liked-songs"))))
                .push(one_line(text::caption(detail)).class(DIM)),
        )
        .push(icon::from_name("go-next-symbolic").size(16));
    button::custom(row)
        .padding([6, 8])
        .width(Length::Fill)
        .class(theme::Button::AppletMenu)
        .on_press(Message::Browse(Browse::OpenLiked))
        .into()
}

// ---------------------------------------------------------------- setup

fn setup_view(state: &Window) -> Element<'_, Message> {
    let reauth = state.session == Session::NeedsReauth;
    let status = if reauth {
        fl!("session-expired")
    } else {
        fl!("signed-out")
    };

    let (title, lead) = if reauth {
        (fl!("reauth-title"), fl!("reauth-lead"))
    } else {
        (fl!("setup-title"), fl!("setup-lead"))
    };

    let connect_row: Element<'_, Message> = if state.is_connecting() {
        Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(text::body(fl!("waiting-browser-approval")).width(Length::Fill))
            .push(button::standard(fl!("cancel")).on_press(Message::CancelConnect))
            .into()
    } else {
        let can_connect = !state.client_id_draft.trim().is_empty();
        Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(
                text_input("Client ID", state.client_id_draft.as_str())
                    .on_input(Message::ClientIdInput)
                    .on_submit(|_| Message::Connect)
                    .width(Length::Fill),
            )
            .push(
                button::suggested(fl!("connect"))
                    .on_press_maybe(can_connect.then_some(Message::Connect)),
            )
            .into()
    };

    let mut form = Column::new()
        .spacing(10)
        .push(text::title4(title))
        .push(text::body(lead).class(DIM))
        .push(connect_row);
    if let Some(error) = &state.error {
        form = form.push(text::caption(error.as_str()).class(ERROR));
    }

    let mut column = Column::new()
        .spacing(14)
        .push(header(
            one_line(text::caption(status)).class(DIM).into(),
            None,
        ))
        .push(divider::horizontal::default())
        .push(form);
    if !reauth {
        column = column
            .push(divider::horizontal::default())
            .push(instructions(state));
    }
    column.into()
}

fn step<'a>(number: &'static str, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Row::new()
        .spacing(10)
        .push(
            text::body(number)
                .class(theme::Text::Accent)
                .width(Length::Fixed(14.0)),
        )
        .push(container(body).width(Length::Fill))
        .into()
}

fn instructions(state: &Window) -> Element<'_, Message> {
    let copy_icon = if state.copied {
        "object-select-symbolic"
    } else {
        "edit-copy-symbolic"
    };
    let redirect = Row::new()
        .spacing(4)
        .align_y(Alignment::Center)
        .push(
            container(text::monotext(state.config.redirect_uri()))
                .padding([4, 8])
                .class(theme::Container::Card),
        )
        .push(
            button::icon(icon::from_name(copy_icon))
                .tooltip(if state.copied {
                    fl!("copied")
                } else {
                    fl!("copy")
                })
                .on_press(Message::CopyRedirectUri),
        );

    Column::new()
        .spacing(12)
        .push(text::heading(fl!("howto-title")))
        .push(step(
            "1",
            Column::new()
                .spacing(2)
                .push(text::body(fl!("howto-step-app")).class(DIM))
                .push(
                    button::link("developer.spotify.com/dashboard")
                        .trailing_icon(true)
                        .on_press(Message::OpenDashboard),
                ),
        ))
        .push(step(
            "2",
            Column::new()
                .spacing(6)
                .push(text::body(fl!("howto-step-redirect")).class(DIM))
                .push(redirect)
                .push(text::body(fl!("howto-step-web-api")).class(DIM)),
        ))
        .push(step("3", text::body(fl!("howto-step-paste")).class(DIM)))
        .into()
}

// ---------------------------------------------------------------- settings

/// `button::destructive` & co. wrap their label when the row is tight, so the
/// label is built by hand to keep it on one line and let the text beside it give way.
fn nowrap_button<'a>(
    label: String,
    class: theme::Button,
    message: Message,
) -> Element<'a, Message> {
    button::custom(container(text::body(label).wrapping(Wrapping::None)).center_y(Length::Fill))
        .height(32)
        .padding([0, 16])
        .class(class)
        .on_press(message)
        .into()
}

fn account_row<'a>(
    title: String,
    label: String,
    class: theme::Button,
    message: Message,
) -> Element<'a, Message> {
    settings::item_row(vec![
        one_line(text::body(title)).width(Length::Fill).into(),
        nowrap_button(label, class, message),
    ])
    .into()
}

fn playback_row(state: &Window) -> Element<'_, Message> {
    match state.playback {
        Playback::Ready => account_row(
            fl!("playback-linked-as", device = state.device_name.as_str()),
            fl!("playback-unlink"),
            theme::Button::Standard,
            Message::DisablePlayback,
        ),
        Playback::NeedsLogin => account_row(
            fl!("playback-not-linked"),
            fl!("playback-link"),
            theme::Button::Suggested,
            Message::EnablePlayback,
        ),
        Playback::Authorizing => account_row(
            fl!("waiting-browser"),
            fl!("cancel"),
            theme::Button::Standard,
            Message::CancelPlayback,
        ),
        Playback::Missing => settings::item(
            fl!("playback-not-installed"),
            text::caption("just install").class(DIM),
        )
        .into(),
    }
}

fn settings_view(state: &Window) -> Element<'_, Message> {
    let account: Element<'_, Message> = match &state.session {
        Session::Connected(user) => {
            let who = if user.name.is_empty() {
                fl!("your-account")
            } else {
                user.name.clone()
            };
            account_row(
                fl!("signed-in-as", name = who),
                fl!("sign-out"),
                theme::Button::Suggested,
                Message::Logout,
            )
        }
        Session::NeedsReauth => account_row(
            fl!("session-expired"),
            fl!("reconnect"),
            theme::Button::Suggested,
            Message::ShowPlayer,
        ),
        Session::SignedOut => account_row(
            fl!("signed-out"),
            fl!("connect"),
            theme::Button::Suggested,
            Message::ShowPlayer,
        ),
    };

    let client_id = if state.config.client_id.is_empty() {
        "—".to_owned()
    } else {
        mask(&state.config.client_id)
    };

    let mut panel = settings::section()
        .title(fl!("settings-panel"))
        .add(settings::item(
            fl!("settings-show-in-bar"),
            toggler(state.config.show_track).on_toggle(Message::SetShowTrack),
        ));
    if state.config.show_track {
        let labels: Vec<String> = PanelLook::ALL.iter().map(|look| look.label()).collect();
        panel = panel.add(settings::item(
            fl!("settings-style"),
            dropdown(labels, Some(state.config.panel_look.index()), |index| {
                Message::SetPanelLook(PanelLook::ALL[index])
            })
            .width(Length::Fixed(168.0)),
        ));
    }

    let mut column = Column::new()
        .spacing(14)
        .push(header(
            one_line(text::caption(fl!("settings"))).class(DIM).into(),
            None,
        ))
        .push(divider::horizontal::default())
        .push(
            settings::section()
                .title(fl!("settings-account"))
                .add(account)
                .add(settings::item(
                    "Client ID",
                    text::caption(client_id).class(DIM),
                )),
        );
    if matches!(state.session, Session::Connected(_)) {
        column = column.push(
            settings::section()
                .title(fl!("settings-playback"))
                .add(playback_row(state)),
        );
    }
    column.push(panel).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_track_times() {
        assert_eq!(format_time(0), "0:00");
        assert_eq!(format_time(213_573), "3:33");
        assert_eq!(format_time(3_725_000), "1:02:05");
    }

    #[test]
    fn masks_client_ids() {
        assert_eq!(mask("0123456789abcdef"), "0123…cdef");
        assert_eq!(mask("short"), "•••••");
    }
}
