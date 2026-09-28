use cosmic::Element;
use cosmic::iced::advanced::text::{Ellipsize, EllipsizeHeightLimit, Wrapping};
use cosmic::iced::{Alignment, Color, Length, Limits};
use cosmic::theme;
use cosmic::widget::{
    Column, Row, button, container, divider, icon, image, mouse_area, scrollable,
    segmented_control, settings, slider, space, text, text_input, toggler,
};

use crate::browse::{Browse, Load, Source, Tab};
use crate::marquee::marquee;
use crate::spotify::{Device, Entry, EntryKind, Item, ItemKind, Repeat, Session};
use crate::window::{Message, Playback, View, Window};

pub const POPUP_WIDTH: f32 = 400.0;
/// Longer titles scroll instead of widening the panel.
const PANEL_LABEL_WIDTH: f32 = 180.0;
pub const HERO_SIZE: f32 = 96.0;
const HERO_RADIUS: f32 = 10.0;
const VOLUME_SLIDER_WIDTH: f32 = 110.0;
const ROW_ART: f32 = 40.0;
const LIST_HEIGHT: f32 = 300.0;

const LOGO: &[u8] = include_bytes!(
    "../res/icons/hicolor/scalable/apps/io.github.gbazan92.CosmicExtAppletSpotify-symbolic.svg"
);

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

pub fn panel(state: &Window) -> Element<'_, Message> {
    let applet = &state.core.applet;
    let horizontal = applet.is_horizontal();
    let (icon_size, _) = applet.suggested_size(true);
    let (major, minor) = applet.suggested_padding(true);
    let (pad_x, pad_y) = if horizontal {
        (major, minor)
    } else {
        (minor, major)
    };

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

    let mut content = Row::new().spacing(8).align_y(Alignment::Center).push(cover);

    if horizontal
        && state.config.show_track
        && let Some(item) = state.item()
    {
        let label = if item.subtitle.is_empty() {
            item.name.clone()
        } else {
            format!("{}  ·  {}", item.name, item.subtitle)
        };
        content = content.push(marquee(
            applet.text(label).wrapping(Wrapping::None),
            PANEL_LABEL_WIDTH,
        ));
    }

    let button = button::custom(content)
        .padding([pad_y, pad_x])
        .class(theme::Button::AppletIcon)
        .on_press(Message::TogglePopup);

    let area = mouse_area(button)
        .on_right_press(Message::OpenSettings)
        .on_middle_press(Message::PlayPause)
        .on_scroll(Message::Scroll);

    applet.autosize_window(area).into()
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
                .push(text::heading("Spotify"))
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
        (true, Some(device)) if state.is_playing() => format!("Reproduciendo en {}", device.name),
        (true, Some(device)) => format!("En pausa en {}", device.name),
        (true, None) => "En pausa".to_owned(),
        (false, _) if !state.loaded => "Cargando…".to_owned(),
        (false, _) => "Nada sonando".to_owned(),
    };

    let status: Element<'_, Message> = match (&state.library.notice, &state.error) {
        (Some(notice), None) => one_line(text::caption(notice.as_str()))
            .class(theme::Text::Accent)
            .into(),
        _ => status_line(state, status),
    };

    let device_label = device.map_or("Dispositivos", |device| device.name.as_str());
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
                .tooltip("Actualizar")
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
    let (lead, action): (&str, Option<Element<'_, Message>>) = match state.playback {
        Playback::Ready => return None,
        Playback::NeedsLogin => (
            "Aprobalo una vez en el navegador y la música sale por esta compu, sin abrir Spotify.",
            Some(nowrap_button(
                "Activar",
                theme::Button::Suggested,
                Message::EnablePlayback,
            )),
        ),
        Playback::Authorizing => (
            "Esperando que apruebes en el navegador…",
            Some(nowrap_button(
                "Cancelar",
                theme::Button::Standard,
                Message::CancelPlayback,
            )),
        ),
        Playback::Missing => (
            "Falta el reproductor local; reinstalá el applet con «just install».",
            None,
        ),
    };
    let mut row = Row::new()
        .spacing(12)
        .align_y(Alignment::Center)
        .push(icon::from_name("computer-symbolic").size(20))
        .push(
            Column::new()
                .spacing(2)
                .width(Length::Fill)
                .push(text::heading("Escuchá en esta compu"))
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
        Load::Idle | Load::Loading => placeholder_text("Buscando dispositivos…"),
        Load::Failed(error) => text::caption(error.as_str()).class(ERROR).into(),
        Load::Ready(list) if list.is_empty() => {
            placeholder_text(if state.playback == Playback::Ready {
                "El reproductor de esta compu está arrancando; probá de nuevo en unos segundos."
            } else {
                "No hay dispositivos. Activá la reproducción en esta compu o abrí Spotify en otro lado."
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
        _ => "Nada sonando".to_owned(),
    };
    let detail = match shown {
        Some(item) if active => subtitle(item),
        Some(item) => format!("Último: {}", item.name),
        None if state.playback == Playback::Ready => {
            "Elegí algo de abajo para escucharlo acá.".to_owned()
        }
        None => "Iniciá Spotify en algún dispositivo para verlo acá.".to_owned(),
    };

    let mut title_row = Row::new()
        .spacing(6)
        .align_y(Alignment::Center)
        .push(one_line(text::title4(title)).width(Length::Fill));
    if active {
        let saved = state.saved.unwrap_or(false);
        title_row = title_row.push(
            button::icon(icon::from_name("emblem-favorite-symbolic"))
                .selected(saved)
                .tooltip(if saved {
                    "Quitar de tu biblioteca"
                } else {
                    "Guardar en tu biblioteca"
                })
                .on_press(Message::ToggleSaved),
        );
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
        .push(one_line(text::body(detail)).class(DIM))
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
    let active = state.item().is_some();
    let shuffle = player.is_some_and(|player| player.shuffle);
    let repeat = player.map_or(Repeat::Off, |player| player.repeat);

    let small = |name: &'static str, tooltip: &'static str, message: Message, selected: bool| {
        button::icon(icon::from_name(name))
            .selected(selected)
            .tooltip(tooltip)
            .on_press_maybe(active.then_some(message))
    };

    let play_icon = if state.is_playing() {
        "media-playback-pause-symbolic"
    } else {
        "media-playback-start-symbolic"
    };
    let play = button::custom(icon::from_name(play_icon).size(20))
        .padding([6, 16])
        .class(theme::Button::Standard)
        .on_press(Message::PlayPause);

    let repeat_icon = if repeat == Repeat::Track {
        "media-playlist-repeat-song-symbolic"
    } else {
        "media-playlist-repeat-symbolic"
    };
    let repeat_tip = match repeat {
        Repeat::Off => "Repetir: no",
        Repeat::Context => "Repetir: todo",
        Repeat::Track => "Repetir: canción",
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
                .tooltip("Silenciar")
                .on_press_maybe(volume.map(|_| Message::ToggleMute)),
        )
        .push(
            slider(0..=100u8, volume.unwrap_or(0), Message::SetVolume).width(VOLUME_SLIDER_WIDTH),
        );

    Row::new()
        .spacing(2)
        .align_y(Alignment::Center)
        .push(small(
            "media-playlist-shuffle-symbolic",
            "Aleatorio",
            Message::ToggleShuffle,
            shuffle,
        ))
        .push(small(
            "media-skip-backward-symbolic",
            "Anterior",
            Message::Previous,
            false,
        ))
        .push(play)
        .push(small(
            "media-skip-forward-symbolic",
            "Siguiente",
            Message::Next,
            false,
        ))
        .push(small(
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

fn placeholder_text(message: &str) -> Element<'_, Message> {
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
            Source::Liked => ("Tus me gusta", matches!(detail.items, Load::Ready(_))),
            Source::Entry(entry) => (entry.name.as_str(), true),
        };
        column = column.push(
            Row::new()
                .spacing(6)
                .align_y(Alignment::Center)
                .push(
                    button::icon(icon::from_name("go-previous-symbolic"))
                        .tooltip("Volver")
                        .on_press(Message::Browse(Browse::CloseDetail)),
                )
                .push(one_line(text::heading(title)).width(Length::Fill))
                .push(
                    button::suggested("Reproducir")
                        .leading_icon(icon::from_name("media-playback-start-symbolic"))
                        .on_press_maybe(playable.then_some(Message::Browse(Browse::PlayDetail))),
                ),
        );
    } else {
        column = column.push(
            segmented_control::horizontal(&library.tabs)
                .on_activate(|entity| Message::Browse(Browse::SelectTab(entity))),
        );
        if library.tab == Tab::Search {
            column = column.push(
                text_input::search_input("Buscar en Spotify", library.query.as_str())
                    .on_input(|query| Message::Browse(Browse::Query(query)))
                    .on_submit(|_| Message::Browse(Browse::SubmitSearch))
                    .on_clear(Message::Browse(Browse::ClearSearch)),
            );
        }
    }

    column
        .push(scrollable(list_body(state)).height(LIST_HEIGHT))
        .into()
}

fn list_body(state: &Window) -> Element<'_, Message> {
    let library = &state.library;
    if let Some(detail) = &library.detail {
        return entries_or(state, &detail.items, "Esta lista está vacía.", None);
    }
    match library.tab {
        Tab::Search if library.query.trim().is_empty() => entries_or(
            state,
            &library.recent,
            "Todavía no escuchaste nada. Buscá algo para empezar.",
            Some(section_title("Escuchado recientemente")),
        ),
        Tab::Search => match &library.results {
            Load::Idle | Load::Loading => placeholder_text("Buscando…"),
            Load::Failed(error) => placeholder_error(error),
            Load::Ready(groups) if groups.is_empty() => placeholder_text("Sin resultados."),
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
            "Todavía no tenés playlists.",
            Some(liked_row(library.liked_count)),
        ),
        Tab::Podcasts => entries_or(
            state,
            &library.shows,
            "No seguís ningún podcast. Buscá uno y seguilo en Spotify.",
            None,
        ),
        Tab::Books => entries_or(
            state,
            &library.books,
            "No guardaste audiolibros todavía.",
            None,
        ),
    }
}

fn entries_or<'a>(
    state: &'a Window,
    load: &'a Load<Vec<Entry>>,
    empty: &'a str,
    header: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    match load {
        Load::Idle | Load::Loading => placeholder_text("Cargando…"),
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

fn section_title(title: &str) -> Element<'_, Message> {
    container(text::caption_heading(title).class(DIM))
        .padding([8, 8, 4, 8])
        .into()
}

fn group_title(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Track => "Canciones",
        EntryKind::Artist => "Artistas",
        EntryKind::Album => "Álbumes",
        EntryKind::Playlist => "Playlists",
        EntryKind::Show => "Podcasts",
        EntryKind::Episode => "Episodios",
        EntryKind::Chapter => "Capítulos",
        EntryKind::Audiobook => "Audiolibros",
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

    if entry.kind.is_item() {
        if entry.duration_ms > 0 {
            row = row.push(text::caption(format_time(entry.duration_ms)).class(DIM));
        }
        row = row.push(
            button::icon(icon::from_name("list-add-symbolic"))
                .extra_small()
                .tooltip("Agregar a la cola")
                .on_press(Message::Browse(Browse::Queue(entry.clone()))),
        );
    } else {
        row = row.push(
            button::icon(icon::from_name("media-playback-start-symbolic"))
                .extra_small()
                .tooltip("Reproducir")
                .on_press(Message::Browse(Browse::PlayContext(entry.clone()))),
        );
    }

    button::custom(row)
        .padding([6, 8])
        .width(Length::Fill)
        .class(theme::Button::AppletMenu)
        .selected(current)
        .on_press_maybe(
            entry
                .playable
                .then(|| Message::Browse(Browse::Activate(entry.clone()))),
        )
        .into()
}

fn liked_row<'a>(count: u64) -> Element<'a, Message> {
    let detail = if count > 0 {
        format!("Playlist  ·  {count} canciones")
    } else {
        "Playlist".to_owned()
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
                .push(one_line(text::body("Tus me gusta")))
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
        "Sesión vencida"
    } else {
        "Sin conectar"
    };

    let (title, lead) = if reauth {
        (
            "Tu sesión de Spotify venció",
            "Spotify limita cada inicio de sesión a seis meses. Conectate de nuevo y todo sigue como estaba.",
        )
    } else {
        (
            "Conectá tu cuenta de Spotify",
            "Pegá el Client ID de tu app de Spotify y tocá Conectar. Necesitás Spotify Premium.",
        )
    };

    let connect_row: Element<'_, Message> = if state.is_connecting() {
        Row::new()
            .spacing(8)
            .align_y(Alignment::Center)
            .push(text::body("Esperando que apruebes en el navegador…").width(Length::Fill))
            .push(button::standard("Cancelar").on_press(Message::CancelConnect))
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
                button::suggested("Conectar")
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
                .tooltip(if state.copied { "Copiado" } else { "Copiar" })
                .on_press(Message::CopyRedirectUri),
        );

    Column::new()
        .spacing(12)
        .push(text::heading("Cómo obtener tu Client ID"))
        .push(step(
            "1",
            Column::new()
                .spacing(2)
                .push(
                    text::body("Creá una app en el panel de desarrolladores de Spotify.")
                        .class(DIM),
                )
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
                .push(text::body("En Redirect URIs agregá exactamente:").class(DIM))
                .push(redirect)
                .push(text::body("Marcá Web API y guardá los cambios.").class(DIM)),
        ))
        .push(step(
            "3",
            text::body("Copiá el Client ID de la app, pegalo arriba y tocá Conectar.").class(DIM),
        ))
        .into()
}

// ---------------------------------------------------------------- settings

/// `button::destructive` & co. wrap their label when the row is tight, so the
/// label is built by hand to keep it on one line and let the text beside it give way.
fn nowrap_button<'a>(
    label: &'static str,
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
    label: &'static str,
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
            format!("Activa como «{}»", state.device_name),
            "Desactivar",
            theme::Button::Standard,
            Message::DisablePlayback,
        ),
        Playback::NeedsLogin => account_row(
            "Desactivada".to_owned(),
            "Activar",
            theme::Button::Suggested,
            Message::EnablePlayback,
        ),
        Playback::Authorizing => account_row(
            "Esperando al navegador…".to_owned(),
            "Cancelar",
            theme::Button::Standard,
            Message::CancelPlayback,
        ),
        Playback::Missing => {
            settings::item("No instalada", text::caption("just install").class(DIM)).into()
        }
    }
}

fn settings_view(state: &Window) -> Element<'_, Message> {
    let account: Element<'_, Message> = match &state.session {
        Session::Connected(user) => {
            let who = if user.name.is_empty() {
                "tu cuenta".to_owned()
            } else {
                user.name.clone()
            };
            account_row(
                format!("Conectado como {who}"),
                "Cerrar sesión",
                theme::Button::Destructive,
                Message::Logout,
            )
        }
        Session::NeedsReauth => account_row(
            "Sesión vencida".to_owned(),
            "Reconectar",
            theme::Button::Suggested,
            Message::ShowPlayer,
        ),
        Session::SignedOut => account_row(
            "Sin conectar".to_owned(),
            "Conectar",
            theme::Button::Suggested,
            Message::ShowPlayer,
        ),
    };

    let client_id = if state.config.client_id.is_empty() {
        "—".to_owned()
    } else {
        mask(&state.config.client_id)
    };

    let panel = settings::section().title("Panel").add(settings::item(
        "Mostrar título y artista",
        toggler(state.config.show_track).on_toggle(Message::SetShowTrack),
    ));

    let mut column = Column::new()
        .spacing(14)
        .push(header(
            one_line(text::caption("Configuración")).class(DIM).into(),
            None,
        ))
        .push(divider::horizontal::default())
        .push(
            settings::section()
                .title("Cuenta")
                .add(account)
                .add(settings::item(
                    "Client ID",
                    text::caption(client_id).class(DIM),
                )),
        );
    if matches!(state.session, Session::Connected(_)) {
        column = column.push(
            settings::section()
                .title("Reproducción en esta compu")
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
