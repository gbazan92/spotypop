//! User-facing text, kept in the Fluent catalog under `i18n/en/`.

use std::sync::LazyLock;

use i18n_embed::LanguageLoader;
use i18n_embed::fluent::{FluentLanguageLoader, fluent_language_loader};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

pub static LANGUAGE_LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader: FluentLanguageLoader = fluent_language_loader!();
    loader
        .load_fallback_language(&Localizations)
        .expect("the English catalog is embedded");
    // Isolation marks around arguments show up as stray boxes in some fonts.
    loader.set_use_isolating(false);
    loader
});

#[macro_export]
macro_rules! fl {
    ($id:literal) => {{ i18n_embed_fl::fl!($crate::i18n::LANGUAGE_LOADER, $id) }};
    ($id:literal, $($args:expr),* $(,)?) => {{
        i18n_embed_fl::fl!($crate::i18n::LANGUAGE_LOADER, $id, $($args),*)
    }};
}
