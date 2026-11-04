use serde::Serialize;
use serde_json::Value;

use crate::lang;

pub const DEFAULT: &str = "cheerful";
pub const CUSTOM: &str = "custom";

#[derive(Serialize)]
pub struct Lines {
    pub start: String,
    pub stop: String,
    pub idle: String,
    pub back: String,
    pub section: String,
    pub screenshot: String,
    pub pokes: Vec<String>,
    pub chatter: Vec<String>,
}

fn personality(id: &str, language: &str) -> &'static Value {
    let all = &lang::data(language)["personalities"];
    let english = &lang::data("en")["personalities"];
    [&all[id], &english[id], &english[DEFAULT]]
        .into_iter()
        .find(|p| p.is_object())
        .unwrap_or(&english[DEFAULT])
}

pub fn lines(id: &str, language: &str) -> Lines {
    let own = personality(id, language);
    let english = personality(id, "en");
    let text = |key: &str| own[key].as_str().or(english[key].as_str()).unwrap_or("").to_string();
    let list = |key: &str| {
        own[key]
            .as_array()
            .or(english[key].as_array())
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect()
    };

    Lines {
        start: text("start"),
        stop: text("stop"),
        idle: text("idle"),
        back: text("back"),
        section: text("section"),
        screenshot: text("screenshot"),
        pokes: list("pokes"),
        chatter: list("chatter"),
    }
}

pub fn custom(mut base: Lines, own: &Value) -> Lines {
    let text = |key: &str| own[key].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from);
    for (key, line) in [
        ("start", &mut base.start),
        ("stop", &mut base.stop),
        ("idle", &mut base.idle),
        ("back", &mut base.back),
        ("section", &mut base.section),
        ("screenshot", &mut base.screenshot),
    ] {
        if let Some(own) = text(key) {
            *line = own;
        }
    }
    for (key, lines) in [("pokes", &mut base.pokes), ("chatter", &mut base.chatter)] {
        let own: Vec<String> = own[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(String::from)
            .collect();
        if !own.is_empty() {
            *lines = own;
        }
    }
    base
}

pub fn exists(id: &str) -> bool {
    id == CUSTOM || lang::data("en")["personalities"].get(id).is_some()
}

#[derive(Serialize)]
pub struct Choice {
    id: &'static str,
    name: &'static str,
}

pub fn choices(language: &str) -> Vec<Choice> {
    let english = lang::data("en")["personalities"].as_object();
    english
        .into_iter()
        .flatten()
        .map(|(id, _)| Choice {
            id: id.as_str(),
            name: personality(id, language)["name"].as_str().unwrap_or(id),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_every_personality() {
        for language in lang::list().iter().map(|l| serde_json::to_value(l).unwrap()) {
            let code = language["code"].as_str().unwrap().to_string();
            for choice in choices("en") {
                let own = &lang::data(&code)["personalities"][choice.id];
                assert!(own.is_object(), "{code}: {}", choice.id);
                let lines = lines(choice.id, &code);
                assert!(lines.start.contains("{name}"), "{code} {} start", choice.id);
                assert!(lines.section.contains("{section}"), "{code} {} section", choice.id);
                assert_eq!(lines.pokes.len(), 3, "{code} {} pokes", choice.id);
                assert_eq!(lines.chatter.len(), 3, "{code} {} chatter", choice.id);
                for line in [&lines.stop, &lines.idle, &lines.back, &lines.screenshot] {
                    assert!(!line.is_empty(), "{code} {}", choice.id);
                }
            }
        }
    }

    #[test]
    fn custom_lines_fall_back_when_empty() {
        let own = serde_json::json!({"start": " Hi {name} ", "stop": "", "pokes": ["a", " ", "b"], "chatter": []});
        let lines = custom(lines(DEFAULT, "en"), &own);
        assert_eq!(lines.start, "Hi {name}");
        assert_eq!(lines.stop, super::lines(DEFAULT, "en").stop);
        assert_eq!(lines.pokes, ["a", "b"]);
        assert_eq!(lines.chatter, super::lines(DEFAULT, "en").chatter);
    }
}
