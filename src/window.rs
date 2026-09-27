use cosmic::app::Core;
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Length, Limits};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget::{Column, button, text};
use cosmic::{Action, Element, Task};

pub const APP_ID: &str = "io.github.gbazan92.CosmicExtAppletHola";

pub struct Window {
    core: Core,
    popup: Option<Id>,
    clicks: u32,
}

#[derive(Clone, Debug)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    Increment,
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
        (
            Self {
                core,
                popup: None,
                clicks: 0,
            },
            Task::none(),
        )
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Message) -> Task<Action<Self::Message>> {
        match message {
            Message::TogglePopup => return self.toggle_popup(),
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }
            Message::Increment => self.clicks += 1,
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        self.core
            .applet
            .icon_button("face-smile-symbolic")
            .on_press(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        popup_content(&self.core, self.clicks)
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl Window {
    fn toggle_popup(&mut self) -> Task<Action<Message>> {
        if let Some(popup) = self.popup.take() {
            return surface_task(destroy_popup(popup));
        }

        let Some(parent) = self.core.main_window_id() else {
            return Task::none();
        };

        surface_task(app_popup::<Window>(
            |_| Default::default(),
            move |state: &mut Window| {
                let popup = Id::unique();
                let mut settings = state
                    .core
                    .applet
                    .get_popup_settings(parent, popup, None, None, None);
                settings.positioner.size_limits = Limits::NONE
                    .min_width(200.0)
                    .max_width(360.0)
                    .min_height(100.0)
                    .max_height(600.0);
                state.popup = Some(popup);
                settings
            },
            Some(Box::new(|state: &Window| {
                popup_content(&state.core, state.clicks).map(cosmic::Action::App)
            })),
        ))
    }
}

fn popup_content(core: &Core, clicks: u32) -> Element<'_, Message> {
    let content = Column::new()
        .spacing(12)
        .padding(16)
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .push(text::title4("Hola desde COSMIC"))
        .push(text(format!("Clicks: {clicks}")))
        .push(button::suggested("Sumar").on_press(Message::Increment));

    core.applet.popup_container(content).into()
}

fn surface_task(action: cosmic::surface::Action) -> Task<Action<Message>> {
    cosmic::task::message(cosmic::Action::Cosmic(cosmic::app::Action::Surface(
        action,
    )))
}
