//! Winamp-style panel scopes. Each one is exactly as wide as the track title
//! and exactly one icon tall, so the bar never grows. They follow the real
//! audio when this computer plays (see [`feed`]) and loop an animation otherwise.

use std::f32::consts::TAU;

use cosmic::iced::advanced::Renderer as _;
use cosmic::iced::advanced::renderer::Quad;
use cosmic::iced::advanced::widget::{Tree, tree};
use cosmic::iced::advanced::{Layout, Shell, Widget, layout, mouse, renderer};
use cosmic::iced::time::{Duration, Instant};
use cosmic::iced::{Background, Border, Color, Event, Length, Point, Rectangle, Size, window};
use cosmic::{Element, Renderer, Theme};

use crate::feed;

const FRAME: Duration = Duration::from_millis(33);
const COLUMNS: usize = 28;

#[allow(clippy::cast_precision_loss)]
fn px(value: usize) -> f32 {
    value as f32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    /// Columns across the whole width, with a cap that falls after the peak.
    Bars,
    /// A line that runs the width, like an oscilloscope.
    Wave,
    /// The wave drawn as a filled ribbon.
    Fill,
}

pub fn scope<'a, Message: 'a>(
    kind: ScopeKind,
    width: f32,
    height: f32,
    playing: bool,
) -> Element<'a, Message> {
    Element::new(Scope {
        kind,
        width,
        height,
        playing,
    })
}

/// How fast a column drops once the music gets quieter, in heights per second.
const LEVEL_FALL: f32 = 3.0;
const PEAK_FALL: f32 = 0.35;
const LIVE_PEAK_FALL: f32 = 0.6;
const IDLE_LEVEL: f32 = 0.16;

fn tick<Message>(animate: bool, shell: &mut Shell<'_, Message>, now: Instant) {
    if animate {
        shell.request_redraw_at(now + FRAME);
    }
}

/// What the scope shows: live audio when this computer is playing, otherwise
/// a loop that only suggests music.
struct Clock {
    phase: f32,
    at: Option<Instant>,
    live: bool,
    levels: [f32; COLUMNS],
    /// Peak hold for each column; it eases down so the caps trail the bars.
    peaks: [f32; COLUMNS],
    wave: [f32; feed::WAVE],
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            phase: 0.0,
            at: None,
            live: false,
            levels: [0.0; COLUMNS],
            peaks: [0.0; COLUMNS],
            wave: [0.0; feed::WAVE],
        }
    }
}

impl Clock {
    fn advance(&mut self, playing: bool, now: Instant) {
        let step = self
            .at
            .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32());
        let frame = feed::latest();
        self.live = frame.is_some();
        let moving = playing || self.live;
        self.at = moving.then_some(now);
        if playing && !self.live {
            self.phase = (self.phase + step * 1.15).rem_euclid(TAU);
        }

        let columns = COLUMNS;
        let peak_fall = if self.live { LIVE_PEAK_FALL } else { PEAK_FALL };
        for index in 0..columns {
            let target = frame.as_ref().map_or_else(
                || column_level(index, columns, self.phase),
                |frame| band_level(&frame.bands, index, columns),
            );
            let level = &mut self.levels[index];
            *level = if target >= *level || step == 0.0 {
                target
            } else {
                (*level - step * LEVEL_FALL).max(target)
            };
            let level = *level;
            let peak = &mut self.peaks[index];
            if moving && level >= *peak {
                *peak = level;
            } else {
                *peak = (*peak - step * peak_fall).max(if moving { level } else { IDLE_LEVEL });
            }
        }

        let last = px(feed::WAVE - 1);
        for (index, sample) in self.wave.iter_mut().enumerate() {
            *sample = match &frame {
                Some(frame) => frame.wave[index],
                None => wave_sample(px(index) / last, self.phase),
            };
        }
    }
}

/// The loudest band under a column, so no transient gets averaged away.
fn band_level(bands: &[f32; feed::BANDS], index: usize, columns: usize) -> f32 {
    let start = index * feed::BANDS / columns;
    let end = ((index + 1) * feed::BANDS)
        .div_ceil(columns)
        .clamp(start + 1, feed::BANDS);
    bands[start..end].iter().copied().fold(0.0, f32::max)
}

/// A value in `0.0..=1.0`. Every term uses a whole number of cycles, so the
/// picture at the end of a loop is the picture at the start.
fn column_level(index: usize, count: usize, phase: f32) -> f32 {
    let x = px(index) / px(count.max(1));
    let a = (phase.mul_add(2.0, x * TAU)).sin();
    let b = (phase.mul_add(3.0, x * TAU * 2.0)).sin();
    let c = (phase.mul_add(1.0, x * TAU * 3.0)).sin();
    (0.55 + a.mul_add(0.26, b.mul_add(0.12, c * 0.07))).clamp(0.16, 1.0)
}

/// A value in `-1.0..=1.0`. Same whole-cycle rule, and the two ends meet.
fn wave_sample(x: f32, phase: f32) -> f32 {
    (phase.mul_add(1.0, x * TAU)).sin().mul_add(
        0.62,
        (phase.mul_add(2.0, x * TAU * 2.0))
            .sin()
            .mul_add(0.28, (phase.mul_add(3.0, x * TAU * 3.0)).sin() * 0.10),
    )
}

struct Scope {
    kind: ScopeKind,
    width: f32,
    height: f32,
    playing: bool,
}

impl<Message> Widget<Message, Theme, Renderer> for Scope {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.width), Length::Fixed(self.height))
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Clock>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Clock::default())
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.width, self.height))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn cosmic::iced::advanced::Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return;
        };
        let clock = tree.state.downcast_mut::<Clock>();
        clock.advance(self.playing, *now);
        tick(self.playing || clock.live, shell, *now);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let clock = tree.state.downcast_ref::<Clock>();
        let mut color = Color::from(theme.cosmic().accent_color());
        if !self.playing && !clock.live {
            color.a *= 0.45;
        }
        match self.kind {
            ScopeKind::Bars => draw_bars(renderer, bounds, clock, color),
            ScopeKind::Wave => draw_wave(renderer, bounds, &clock.wave, color, false),
            ScopeKind::Fill => draw_wave(renderer, bounds, &clock.wave, color, true),
        }
    }
}

fn draw_bars(renderer: &mut Renderer, bounds: Rectangle, clock: &Clock, color: Color) {
    let slot = bounds.width / px(COLUMNS);
    let bar = (slot * 0.72).max(1.0);
    let gap = slot - bar;
    let width = bar.round().max(1.0);
    let step = bar + gap;
    let origin = bounds.x + gap / 2.0;
    for index in 0..COLUMNS {
        // One piece up to the held peak. A separate cap leaves a hairline gap
        // under the tip of some columns.
        let height = (bounds.height * clock.peaks[index].max(clock.levels[index]))
            .round()
            .max(2.0);
        column_quad(
            renderer,
            color,
            Rectangle::new(
                Point::new(
                    (origin + px(index) * step).round(),
                    (bounds.y + bounds.height - height).round(),
                ),
                Size::new(width, height),
            ),
            1.0,
        );
    }
}

fn column_quad(renderer: &mut Renderer, color: Color, bounds: Rectangle, radius: f32) {
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        },
        Background::Color(color),
    );
}

/// `points` evenly spread from the left edge (`x = 0`) to the right (`x = 1`).
fn sample_at(points: &[f32; feed::WAVE], x: f32) -> f32 {
    let position = x.clamp(0.0, 1.0) * px(feed::WAVE - 1);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lower = (position.floor() as usize).min(feed::WAVE - 2);
    let t = position - px(lower);
    (points[lower + 1] - points[lower]).mul_add(t, points[lower])
}

fn draw_wave(
    renderer: &mut Renderer,
    bounds: Rectangle,
    points: &[f32; feed::WAVE],
    color: Color,
    fill: bool,
) {
    // One sample per pixel, plus a little overlap, so slices never leave a seam.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = (bounds.width.round() as usize).clamp(48, 180);
    let step = bounds.width / px(count);
    let mid = bounds.y + bounds.height / 2.0;
    let amp = bounds.height * 0.40;
    let last = px(count - 1);
    for index in 0..count {
        let y = mid - sample_at(points, px(index) / last) * amp;
        let y0 = mid - sample_at(points, px(index.saturating_sub(1)) / last) * amp;
        let slice = if fill {
            let top = y.min(mid);
            Rectangle::new(
                Point::new((bounds.x + px(index) * step).round(), top.round()),
                Size::new(step + 1.0, (y - mid).abs().round().max(1.5)),
            )
        } else {
            let top = y.min(y0);
            Rectangle::new(
                Point::new((bounds.x + px(index) * step).round(), top.round()),
                Size::new(step + 1.0, (y - y0).abs().round().max(2.5)),
            )
        };
        column_quad(renderer, color, slice, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cycle_ends_where_it_started() {
        for index in 0..COLUMNS {
            let start = column_level(index, COLUMNS, 0.0);
            let end = column_level(index, COLUMNS, TAU);
            assert!((start - end).abs() < 1e-4, "column {index}");
        }
        for step in 0..8 {
            let x = px(step) / 8.0;
            assert!((wave_sample(x, 0.0) - wave_sample(x, TAU)).abs() < 1e-4);
        }
        assert!((wave_sample(0.0, 1.3) - wave_sample(1.0, 1.3)).abs() < 1e-4);
    }

    #[test]
    fn every_band_reaches_a_column() {
        for columns in [COLUMNS, COLUMNS / 2] {
            for band in 0..feed::BANDS {
                let mut bands = [0.0; feed::BANDS];
                bands[band] = 1.0;
                let lit = (0..columns).filter(|&i| band_level(&bands, i, columns) > 0.0);
                assert!(lit.count() >= 1, "band {band} with {columns} columns");
            }
        }
    }

    #[test]
    fn waves_interpolate_between_points() {
        let mut points = [0.0; feed::WAVE];
        points[feed::WAVE - 1] = 1.0;
        assert!(sample_at(&points, 0.0).abs() < f32::EPSILON);
        assert!((sample_at(&points, 1.0) - 1.0).abs() < f32::EPSILON);
    }
}
