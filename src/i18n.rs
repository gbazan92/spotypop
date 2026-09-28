//! Fluent catalogs under `i18n/`, picked from the desktop's languages.

use std::sync::LazyLock;

use i18n_embed::fluent::{FluentLanguageLoader, fluent_language_loader};
use i18n_embed::{DefaultLocalizer, DesktopLanguageRequester, LanguageLoader, Localizer};
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

pub fn init() {
    let requested = DesktopLanguageRequester::requested_languages();
    let localizer = DefaultLocalizer::new(&*LANGUAGE_LOADER, &Localizations);
    if let Err(error) = localizer.select(&requested) {
        eprintln!("could not load translations: {error}");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    fn ids(catalog: &str) -> BTreeSet<&str> {
        catalog
            .lines()
            .filter(|line| line.starts_with(|c: char| c.is_ascii_lowercase()))
            .filter_map(|line| line.split_once(" =").map(|(id, _)| id))
            .collect()
    }

    #[test]
    fn every_language_has_every_message() {
        let english = ids(include_str!("../i18n/en/spotypop.ftl"));
        let spanish = ids(include_str!("../i18n/es/spotypop.ftl"));
        assert!(!english.is_empty());
        assert_eq!(english, spanish);
    }
}

#[macro_export]
macro_rules! fl {
    ($id:literal) => {{ i18n_embed_fl::fl!($crate::i18n::LANGUAGE_LOADER, $id) }};
    ($id:literal, $($args:expr),* $(,)?) => {{
        i18n_embed_fl::fl!($crate::i18n::LANGUAGE_LOADER, $id, $($args),*)
    }};
}
