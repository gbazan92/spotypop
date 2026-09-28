mod art;
mod browse;
mod browser;
mod config;
mod feed;
mod i18n;
mod look;
mod marquee;
mod player;
mod spotify;
mod ui;
mod window;

fn main() -> cosmic::iced::Result {
    // reqwest is built without a bundled crypto provider; ring only needs a C compiler.
    let _ = rustls::crypto::ring::default_provider().install_default();
    i18n::init();
    cosmic::applet::run::<window::Window>(())
}
