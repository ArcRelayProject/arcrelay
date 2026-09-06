//! Native menus and notifications use the same catalogs as the web UI.
use crate::settings::LanguagePreference;
use std::{collections::HashMap, sync::LazyLock};

type Catalog = HashMap<String, String>;
static CATALOGS: LazyLock<[Catalog; 7]> = LazyLock::new(|| {
    [
        include_str!("../frontend/src/locales/en.json"),
        include_str!("../frontend/src/locales/ja.json"),
        include_str!("../frontend/src/locales/ko.json"),
        include_str!("../frontend/src/locales/de.json"),
        include_str!("../frontend/src/locales/fr.json"),
        include_str!("../frontend/src/locales/es.json"),
        include_str!("../frontend/src/locales/pt.json"),
    ]
    .map(|json| serde_json::from_str(json).expect("valid bundled translation catalog"))
});

pub fn translate(language: LanguagePreference, source: &str, english: &str) -> String {
    let index = match language.resolve() {
        LanguagePreference::ZhCn => return source.to_owned(),
        LanguagePreference::JaJp => 1,
        LanguagePreference::KoKr => 2,
        LanguagePreference::DeDe => 3,
        LanguagePreference::FrFr => 4,
        LanguagePreference::EsEs => 5,
        LanguagePreference::PtBr => 6,
        LanguagePreference::EnUs | LanguagePreference::System => 0,
    };
    CATALOGS[index]
        .get(source)
        .map(String::as_str)
        .unwrap_or(english)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_translation_needs_no_runtime() {
        assert_eq!(
            translate(LanguagePreference::DeDe, "保存", "Save"),
            "Speichern"
        );
        assert_eq!(translate(LanguagePreference::ZhCn, "保存", "Save"), "保存");
        assert_eq!(
            translate(LanguagePreference::JaJp, "unknown", "fallback"),
            "fallback"
        );
    }
}
