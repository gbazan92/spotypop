//! Text that scrolls left in a loop when it does not fit, like a car stereo.

use cosmic::iced::advanced::Renderer as _;
use cosmic::iced::advanced::widget::{Tree, tree};
use cosmic::iced::advanced::{Clipboard, Layout, Shell, Widget, layout, renderer};
use cosmic::iced::time::{Duration, Instant};
use cosmic::iced::{Event, Length, Rectangle, Size, Vector, mouse, window};
use cosmic::{Element, Renderer, Theme};

/// Pixels per second.
const SPEED: f32 = 30.0;
/// Space between the end of the text and its next lap.
const GAP: f32 = 40.0;
/// Rest at the start of every lap so the beginning can be read.
const PAUSE: f32 = 2.0;
/// About 30 fps: smooth enough for slow text, cheap for an always-on panel.
const FRAME: Duration = Duration::from_millis(33);

pub struct Marquee<'a, Message> {
    content: Element<'a, Message>,
    /// Caps the width. `None` means use whatever width the parent gives.
    max_width: Option<f32>,
}

/// `content` must not wrap, so its natural width is the whole line.
pub fn marquee<'a, Message>(
    content: impl Into<Element<'a, Message>>,
    max_width: f32,
) -> Marquee<'a, Message> {
    Marquee {
        content: content.into(),
        max_width: Some(max_width),
    }
}

/// Same scrolling line, stretched to the width it is given.
pub fn marquee_fill<'a, Message>(content: impl Into<Element<'a, Message>>) -> Marquee<'a, Message> {
    Marquee {
        content: content.into(),
        max_width: None,
    }
}

#[derive(Default)]
struct State {
    started: Option<Instant>,
    offset: f32,
}

fn content_width(layout: Layout<'_>) -> f32 {
    layout
        .children()
        .next()
        .map_or(0.0, |content| content.bounds().width)
}

impl<Message> Widget<Message, Theme, Renderer> for Marquee<'_, Message> {
    fn size(&self) -> Size<Length> {
        let width = if self.max_width.is_some() {
            Length::Shrink
        } else {
            Length::Fill
        };
        Size::new(width, Length::Shrink)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let unbounded =
            layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, limits.max().height));
        let content =
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &unbounded);
        let natural = content.size();
        let offered = limits.max().width;
        let width = match self.max_width {
            Some(max) => natural.width.min(max).min(offered),
            None if offered.is_finite() => offered,
            None => natural.width,
        };
        layout::Node::with_children(Size::new(width, natural.height), vec![content])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return;
        };
        let state = tree.state.downcast_mut::<State>();
        let natural = content_width(layout);
        if natural <= layout.bounds().width + 0.5 {
            state.started = None;
            state.offset = 0.0;
            return;
        }
        let started = *state.started.get_or_insert(*now);
        let lap = natural + GAP;
        let cycle = PAUSE + lap / SPEED;
        let elapsed = now.saturating_duration_since(started).as_secs_f32() % cycle;
        state.offset = (elapsed - PAUSE).max(0.0) * SPEED;
        // Nothing moves during the rest, so sleep through it.
        let next = if elapsed < PAUSE {
            Duration::from_secs_f32(PAUSE - elapsed).max(FRAME)
        } else {
            FRAME
        };
        shell.request_redraw_at(*now + next);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let Some(content_layout) = layout.children().next() else {
            return;
        };
        let bounds = layout.bounds();
        let natural = content_layout.bounds().width;
        let content = self.content.as_widget();
        let child = &tree.children[0];
        if natural <= bounds.width + 0.5 {
            content.draw(
                child,
                renderer,
                theme,
                style,
                content_layout,
                cursor,
                viewport,
            );
            return;
        }
        let offset = tree.state.downcast_ref::<State>().offset;
        renderer.with_layer(bounds, |renderer| {
            for shift in [-offset, natural + GAP - offset] {
                renderer.with_translation(Vector::new(shift, 0.0), |renderer| {
                    content.draw(
                        child,
                        renderer,
                        theme,
                        style,
                        content_layout,
                        cursor,
                        viewport,
                    );
                });
            }
        });
    }
}

impl<'a, Message: 'a> From<Marquee<'a, Message>> for Element<'a, Message> {
    fn from(marquee: Marquee<'a, Message>) -> Self {
        Element::new(marquee)
    }
}
