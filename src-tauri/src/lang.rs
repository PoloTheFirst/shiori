use std::sync::LazyLock;

use serde::Serialize;
use serde_json::Value;
use windows::Win32::Globalization::GetUserDefaultLocaleName;

const FILES: [(&str, &str); 8] = [
    ("en", include_str!("../../ui/locales/en.json")),
    ("fr", include_str!("../../ui/locales/fr.json")),
    ("es", include_str!("../../ui/locales/es.json")),
    ("pt", include_str!("../../ui/locales/pt.json")),
    ("ru", include_str!("../../ui/locales/ru.json")),
    ("ja", include_str!("../../ui/locales/ja.json")),
    ("vi", include_str!("../../ui/locales/vi.json")),
    ("zh-TW", include_str!("../../ui/locales/zh-TW.json")),
];

static LANGUAGES: LazyLock<Vec<(&'static str, Value)>> = LazyLock::new(|| {
    FILES
        .iter()
        .map(|(code, text)| (*code, serde_json::from_str(text).expect("language file")))
        .collect()
});

pub fn data(code: &str) -> &'static Value {
    let languages: &'static Vec<(&'static str, Value)> = &LANGUAGES;
    languages
        .iter()
        .find(|(c, _)| *c == code)
        .map_or(&languages[0].1, |(_, value)| value)
}

fn system_language() -> &'static str {
    let mut buffer = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(&mut buffer) };
    let locale = String::from_utf16_lossy(&buffer[..len.max(1) as usize - 1]).to_lowercase();
    FILES
        .iter()
        .map(|(code, _)| *code)
        .find(|code| locale.starts_with(&code.to_ascii_lowercase()))
        .unwrap_or("en")
}

pub fn resolve(setting: &str) -> &'static str {
    FILES
        .iter()
        .map(|(code, _)| *code)
        .find(|code| *code == setting)
        .unwrap_or_else(system_language)
}

pub fn tr(text: &'static str, language: &str) -> &'static str {
    data(language)["app"]
        .get(text)
        .and_then(Value::as_str)
        .unwrap_or(text)
}

#[derive(Serialize)]
pub struct Language {
    code: &'static str,
    name: &'static str,
    flag: &'static str,
}

pub fn codes() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(code, _)| *code)
}

pub fn list() -> Vec<Language> {
    FILES
        .iter()
        .map(|(code, _)| {
            let data = data(code);
            Language {
                code,
                name: data["name"].as_str().unwrap_or(code),
                flag: data["flag"].as_str().unwrap_or(""),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_file_is_complete() {
        for (code, _) in FILES {
            let data = data(code);
            assert!(data["name"].is_string() && data["flag"].is_string(), "{code}: name/flag");
            assert!(data["ui"].is_object() && data["app"].is_object(), "{code}: ui/app");
            if code != "en" {
                for key in ["Reading", "Total read: {}", "Today: {}", "{h}h {mm}m", "View on VNDB", "Hide this mascot"] {
                    assert!(data["app"][key].is_string(), "{code}: app {key}");
                }
            }
        }
        assert_eq!(resolve("fr"), "fr");
        assert_eq!(tr("Reading", "en"), "Reading");
    }
}
