mod art;
mod browse;
mod browser;
mod config;
mod marquee;
mod player;
mod spotify;
mod ui;
mod window;

fn main() -> cosmic::iced::Result {
    // reqwest is built without a bundled crypto provider; ring only needs a C compiler.
    let _ = rustls::crypto::ring::default_provider().install_default();
    cosmic::applet::run::<window::Window>(())
}
