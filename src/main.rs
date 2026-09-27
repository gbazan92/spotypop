mod window;

fn main() -> cosmic::iced::Result {
    cosmic::applet::run::<window::Window>(())
}
