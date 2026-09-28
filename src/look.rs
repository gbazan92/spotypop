//! Winamp-style panel scopes. Each one is exactly as wide as the track title
//! and exactly one icon tall, so the bar never grows.

use std::f32::consts::TAU;

use cosmic::iced::advanced::Renderer as _;
use cosmic::iced::advanced::renderer::Quad;
use cosmic::iced::advanced::widget::{Tree, tree};
use cosmic::iced::advanced::{Layout, Shell, Widget, layout, mouse, renderer};
use cosmic::iced::time::{Duration, Instant};
use cosmic::iced::{Background, Border, Color, Event, Length, Point, Rectangle, Size, window};
use cosmic::{Element, Renderer, Theme};

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
    /// The same columns, mirrored from the middle.
    Mirror,
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

fn tick<Message>(playing: bool, shell: &mut Shell<'_, Message>, now: Instant) {
    if playing {
        shell.request_redraw_at(now + FRAME);
    }
}

struct Clock {
    phase: f32,
    at: Option<Instant>,
    /// Peak hold for each column; it eases down so the caps trail the bars.
    peaks: [f32; COLUMNS],
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            phase: 0.0,
            at: None,
            peaks: [0.0; COLUMNS],
        }
    }
}

impl Clock {
    fn advance(&mut self, playing: bool, now: Instant, columns: usize) {
        let step = self
            .at
            .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32());
        if playing {
            self.phase = (self.phase + step * 1.15).rem_euclid(TAU);
            self.at = Some(now);
        } else {
            self.at = None;
        }
        for index in 0..columns.min(COLUMNS) {
            let level = column_level(index, columns, self.phase);
            let peak = &mut self.peaks[index];
            if playing && level >= *peak {
                *peak = level;
            } else {
                *peak = (*peak - step * 0.35).max(if playing { level } else { 0.16 });
            }
        }
    }
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
        let columns = if self.kind == ScopeKind::Mirror {
            COLUMNS / 2
        } else {
            COLUMNS
        };
        tree.state
            .downcast_mut::<Clock>()
            .advance(self.playing, *now, columns);
        tick(self.playing, shell, *now);
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
        if !self.playing {
            color.a *= 0.45;
        }
        match self.kind {
            ScopeKind::Bars => draw_bars(renderer, bounds, clock, color, false),
            ScopeKind::Mirror => draw_bars(renderer, bounds, clock, color, true),
            ScopeKind::Wave => draw_wave(renderer, bounds, clock.phase, color, false),
            ScopeKind::Fill => draw_wave(renderer, bounds, clock.phase, color, true),
        }
    }
}

fn draw_bars(
    renderer: &mut Renderer,
    bounds: Rectangle,
    clock: &Clock,
    color: Color,
    mirror: bool,
) {
    let count = if mirror { COLUMNS / 2 } else { COLUMNS };
    let columns = if mirror { COLUMNS } else { count };
    let slot = bounds.width / px(columns);
    let bar = (slot * 0.72).max(1.0);
    let gap = slot - bar;
    for index in 0..count {
        let level = column_level(index, count, clock.phase);
        let peak = clock.peaks[index];
        paint_column(
            renderer, bounds, color, index, mirror, bar, gap, level, peak,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_column(
    renderer: &mut Renderer,
    bounds: Rectangle,
    color: Color,
    index: usize,
    mirror: bool,
    bar: f32,
    gap: f32,
    level: f32,
    peak: f32,
) {
    // One piece up to the held peak. A separate cap leaves a hairline gap
    // under the tip of some columns.
    let height = (bounds.height * peak.max(level)).round().max(2.0);
    let width = bar.round().max(1.0);
    let step = bar + gap;
    let slots = if mirror {
        let center = bounds.x + bounds.width / 2.0 + gap / 2.0;
        [center - px(index + 1) * step, center + px(index) * step]
    } else {
        let origin = bounds.x + gap / 2.0;
        [origin + px(index) * step, f32::NAN]
    };
    for x in slots {
        if !x.is_finite() {
            continue;
        }
        column_quad(
            renderer,
            color,
            Rectangle::new(
                Point::new(x.round(), (bounds.y + bounds.height - height).round()),
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

fn draw_wave(renderer: &mut Renderer, bounds: Rectangle, phase: f32, color: Color, fill: bool) {
    // One sample per pixel, plus a little overlap, so slices never leave a seam.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = (bounds.width.round() as usize).clamp(48, 180);
    let step = bounds.width / px(count);
    let mid = bounds.y + bounds.height / 2.0;
    let amp = bounds.height * 0.40;
    for index in 0..count {
        let x = px(index) / px(count);
        let y = mid - wave_sample(x, phase) * amp;
        let previous = wave_sample(
            if index == 0 {
                1.0
            } else {
                px(index - 1) / px(count)
            },
            phase,
        );
        let y0 = mid - previous * amp;
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
}
