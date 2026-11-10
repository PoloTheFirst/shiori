use std::sync::LazyLock;

use fancy_regex::{Captures, Regex};

pub struct Rule {
    regex: Regex,
    label: String,
    pub section_type: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct TitleInfo {
    pub section_type: String,
    pub section_label: String,
}

const ROMAN: &str = "IVXLCDM";

impl Rule {
    pub fn new(pattern: &str, label: &str, section_type: &str) -> Option<Rule> {
        let regex = Regex::new(&format!("(?i){pattern}")).ok()?;
        Some(Rule {
            regex,
            label: label.to_string(),
            section_type: section_type.to_string(),
        })
    }

    fn matches(&self, text: &str) -> Option<TitleInfo> {
        let caps = self.regex.captures(text).ok()??;
        let group = |names: &[&str]| -> String {
            names
                .iter()
                .find_map(|n| caps.name(n).map(|m| m.as_str()).filter(|s| !s.is_empty()))
                .unwrap_or("")
                .to_string()
        };
        let n = normalize_ordinal(&group(&["n", "n2"]));
        let label = titlecase(group(&["label", "label2"]).trim_matches(|c| " -:,".contains(c)));
        let text_out = fill_template(&self.label, &caps, &n, &label);
        Some(TitleInfo {
            section_type: self.section_type.clone(),
            section_label: squash(&text_out),
        })
    }
}

static FIELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{(\w+)\}").unwrap());

fn fill_template(template: &str, caps: &Captures<'_, str>, n: &str, label: &str) -> String {
    FIELD
        .replace_all(template, |c: &Captures<'_, str>| {
            match c.get(1).map_or("", |m| m.as_str()) {
                "n" => n.to_string(),
                "label" => label.to_string(),
                other => caps
                    .name(other)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
            }
        })
        .into_owned()
}

static DEFAULT_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let ch = r"(?:chapter|chapitre|chap\.?|ch\.?|episode|épisode|ep\.?|act|acte|scene|sc[eè]ne|part|partie|volume|vol\.?)";
    let num = format!(r"(?P<n>[0-9]{{1,3}}|[{ROMAN}]{{1,7}})");
    let rules: Vec<(String, &str, &str)> = vec![
        (r"\b(prologue|prolog|プロローグ|序章|序幕)\b".into(), "Prologue", "prologue"),
        (r"\b(epilogue|epilog|エピローグ|終章)\b".into(), "Epilogue", "ending"),
        (r"\b(opening|intro(?:duction)?|オープニング|導入)\b".into(), "Opening", "prologue"),
        (
            r"\b(?P<label>[\w'’\. ]+?)\s*(?:route|ルート|편|線|の物語)\b|\b(?:route|arc|path)\s*[:\-]?\s*(?P<label2>[\w'’\. ]+)".into(),
            "{label} Route",
            "route",
        ),
        (format!(r"(?P<label>[^,]+?)\s*,\s*{ch}\s*{num}\b"), "{label} — Chapter {n}", "chapter"),
        (format!(r"\b{ch}\s*{num}\s*[:：,\-–—]\s*(?P<label>[^\W_].*)$"), "Chapter {n} — {label}", "chapter"),
        (r"\b(?:day|jour)\s*(?P<n>[0-9]{1,3})\s*[:：,\-–—]\s*(?P<label>[^\W_].*)$".into(), "Day {n} — {label}", "chapter"),
        (
            format!(r"\b{ch}\s*{num}\b|第\s*(?P<n2>[0-9一二三四五六七八九十百]+)\s*[章話幕節]"),
            "Chapter {n}",
            "chapter",
        ),
        (r"\b(common route|common|共通(?:ルート)?|共通線)\b".into(), "Common Route", "route"),
        (r"\b(true route|true end(?:ing)?|グランドルート|真ルート)\b".into(), "True Route", "route"),
        (r"\b(?P<label>good|bad|normal|happy|true|grand|harem)\s+end(?:ing)?\b".into(), "{label} Ending", "ending"),
        (r"\b(ending|end|エンディング|エンド)\b".into(), "Ending", "ending"),
        (r"\b(?:day|jour|days?)\s*(?P<n>[0-9]{1,3})\b|(?P<n2>[0-9]{1,3})\s*日目".into(), "Day {n}", "chapter"),
        (r"(?P<label>[\w'’぀-ヿ一-鿿]+?)\s*編\b".into(), "{label} Arc", "route"),
        (r"[～~]\s*(?P<label>.+?)\s*[～~]".into(), "{label}", "free"),
    ];
    rules
        .into_iter()
        .map(|(p, l, t)| Rule::new(&p, l, t).expect("default title rule"))
        .collect()
});

pub fn user_rules(extra: &[serde_json::Value]) -> Vec<Rule> {
    extra
        .iter()
        .filter_map(|item| {
            let pattern = item.get("pattern")?.as_str()?;
            let label = item
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("{label}");
            let kind = item
                .get("section_type")
                .and_then(|v| v.as_str())
                .unwrap_or("free");
            Rule::new(pattern, label, kind)
        })
        .collect()
}

fn regex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if r"\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

fn flex_pattern(name: &str) -> String {
    let collapsed: String = name
        .chars()
        .map(|c| if is_word(c) { c } else { ' ' })
        .collect::<String>()
        .to_lowercase();
    collapsed
        .split_whitespace()
        .map(|tok| {
            if tok == "o" || tok == "wo" {
                "w?o".to_string()
            } else {
                regex_escape(tok)
            }
        })
        .collect::<Vec<_>>()
        .join(r"[\s\W_]+")
}

const STRIP_CHARS: &str = " -–—|:：·•\u{3000}";

pub fn strip_game_name(title: &str, names: &[&str]) -> (String, bool) {
    let mut remainder = title.trim().to_string();
    let mut variants: Vec<String> = Vec::new();
    for name in names.iter().filter(|n| !n.is_empty()) {
        variants.push(name.to_string());
        for sep in [":", " - ", "~", "～"] {
            if name.contains(sep) {
                variants.push(name.split(sep).next().unwrap_or("").to_string());
            }
        }
    }
    variants.sort_by_key(|v| std::cmp::Reverse(v.chars().count()));

    let mut found = false;
    let mut seen: Vec<String> = Vec::new();
    for variant in variants {
        let pat = flex_pattern(&variant);
        if pat.is_empty() || seen.contains(&pat) {
            continue;
        }
        seen.push(pat.clone());
        let Ok(re) = Regex::new(&format!("(?i){pat}")) else {
            continue;
        };
        if let Ok(Some(m)) = re.find(&remainder) {
            remainder = format!("{} {}", &remainder[..m.start()], &remainder[m.end()..]);
            found = true;
            break;
        }
    }
    let remainder = squash_spaces(&remainder)
        .trim_matches(|c| STRIP_CHARS.contains(c))
        .to_string();
    (remainder, found)
}

static VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:ver\.?\s?|v)?\d+[.;]\d+(?:[.;]\d+)*[a-z]?\b").unwrap());
static EMPTY_BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\[(（【]\s*[\])）】]").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());

fn drop_lost_brackets(text: &str) -> String {
    let text = text.trim();
    match text.strip_prefix('?') {
        Some(inner) if inner.starts_with(char::is_alphanumeric) => {
            inner.strip_suffix('?').unwrap_or(inner).trim().to_string()
        }
        _ => text.to_string(),
    }
}

fn squash_spaces(text: &str) -> String {
    SPACES.replace_all(text, " ").into_owned()
}

fn squash(text: &str) -> String {
    squash_spaces(text)
        .trim_matches(|c| " -–—".contains(c))
        .to_string()
}

pub fn parse(title: &str, game_name: &str, other_names: &[String], user: &[Rule]) -> TitleInfo {
    let names: Vec<&str> = [game_name]
        .into_iter()
        .chain(other_names.iter().map(String::as_str))
        .collect();
    let (remainder, found) = strip_game_name(title, &names);
    let remainder = VERSION.replace_all(&remainder, " ");
    let remainder = EMPTY_BRACKETS.replace_all(&remainder, "");
    let remainder = squash(&remainder)
        .trim_matches(|c| STRIP_CHARS.contains(c))
        .to_string();
    let remainder = drop_lost_brackets(&remainder);
    if loose(&remainder).is_empty() {
        return TitleInfo::default();
    }
    for rule in user.iter().chain(DEFAULT_RULES.iter()) {
        if let Some(info) = rule.matches(&remainder) {
            if !info.section_label.is_empty() {
                return info;
            }
        }
    }
    if found && loose(&remainder) != loose(game_name) {
        return TitleInfo {
            section_type: "free".into(),
            section_label: squash(&titlecase(&remainder)),
        };
    }
    TitleInfo::default()
}

pub fn loose(text: &str) -> String {
    text.chars()
        .filter(|c| is_word(*c))
        .collect::<String>()
        .to_lowercase()
}

fn titlecase(text: &str) -> String {
    let has_upper = text.chars().any(char::is_uppercase);
    let has_lower = text.chars().any(char::is_lowercase);
    if has_upper != has_lower {
        text.split(' ')
            .map(|w| {
                let mut cs = w.chars();
                match cs.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        text.to_string()
    }
}

fn normalize_ordinal(token: &str) -> String {
    let token = token.trim();
    if token.is_empty() || token.chars().all(|c| c.is_ascii_digit()) {
        return token.to_string();
    }
    if token.to_uppercase().chars().all(|c| ROMAN.contains(c)) {
        return roman_to_int(&token.to_uppercase()).to_string();
    }
    if token
        .chars()
        .any(|c| kanji_digit(c).is_some() || c == '十' || c == '百')
    {
        return kanji_to_int(token).to_string();
    }
    token.to_string()
}

fn roman_to_int(s: &str) -> i32 {
    let val = |c| match c {
        'I' => 1,
        'V' => 5,
        'X' => 10,
        'L' => 50,
        'C' => 100,
        'D' => 500,
        'M' => 1000,
        _ => 0,
    };
    let (mut total, mut prev) = (0, 0);
    for c in s.chars().rev() {
        let v = val(c);
        total += if v < prev { -v } else { v };
        prev = prev.max(v);
    }
    total
}

fn kanji_digit(c: char) -> Option<u32> {
    "一二三四五六七八九"
        .chars()
        .position(|k| k == c)
        .map(|i| i as u32 + 1)
}

fn kanji_to_int(s: &str) -> u32 {
    let (mut section, mut number) = (0, 0);
    for c in s.chars() {
        if let Some(d) = kanji_digit(c) {
            number = d;
        } else if c == '十' {
            section += number.max(1) * 10;
            number = 0;
        } else if c == '百' {
            section += number.max(1) * 100;
            number = 0;
        } else if let Some(d) = c.to_digit(10) {
            number = number * 10 + d;
        } else {
            number = 0;
        }
    }
    section + number
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_match_python() {
        let cases = [
            (
                "Senren * Banka - Chapter 4: Yoshino Route",
                "Senren * Banka",
                "Yoshino Route",
            ),
            ("Hapymaher Prologue", "Hapymaher", "Prologue"),
            ("Some VN - Chapitre IV", "Some VN", "Chapter 4"),
            ("ゲーム 第三章", "ゲーム", "Chapter 3"),
            ("Game - Yoshino Route", "Game", "Yoshino Route"),
            ("Game - good ending", "Game", "Good Ending"),
            ("Game", "Game", ""),
            ("Sayonara wo Oshiete ~Day 3~", "Sayonara o Oshiete", "Day 3"),
            (
                "Subarashiki Hibi ~Furenzoku Sonzai~ - Chapter 2, Jabberwocky",
                "Subarashiki Hibi",
                "Chapter 2 — Jabberwocky",
            ),
            (
                "Little Busters! - Common Route",
                "Little Busters!",
                "Common Route",
            ),
            (
                "Riddle Joker ver1.02 - 共通ルート",
                "Riddle Joker",
                "共通 Route",
            ),
            (
                "Game - Episode 12 - The Fall",
                "Game",
                "Chapter 12 — The Fall",
            ),
            ("Game - Act III", "Game", "Chapter 3"),
            ("Game 3日目", "Game", "Day 3"),
            ("Making*Lovers  Epilogue", "Making*Lovers", "Epilogue"),
            ("Game - Chapter 4", "Game", "Chapter 4"),
            ("Some English Title - Our Secret Base", "Japanese Title", ""),
            ("Unknown Title - Chapter 2", "Japanese Title", "Chapter 2"),
            (
                "Hamidashi Creative - Ver1.00 ?New Gacha Starts Today?",
                "Hamidashi Creative",
                "New Gacha Starts Today",
            ),
            ("Game - Is This Love?", "Game", "Is This Love?"),
        ];
        for (title, game, want) in cases {
            assert_eq!(
                parse(title, game, &[], &[]).section_label,
                want,
                "title: {title}"
            );
        }
    }

    #[test]
    fn sections_use_other_titles() {
        let other = vec!["HimaNatsu: Of Churches, Sunflowers, and Long Summers".to_string()];
        let title = "HimaNatsu: Of Churches, Sunflowers, and Long Summers - Our Secret Base";
        let info = parse(title, "Himawari no Kyoukai to Nagai Natsuyasumi", &other, &[]);
        assert_eq!(info.section_label, "Our Secret Base");
    }

    #[test]
    fn leftover_punctuation_is_not_a_section() {
        let other = vec!["Re:LieF ～親愛なるあなたへ～".to_string()];
        for title in ["Re:LieF ～親愛なるあなたへ～", "Re: LieF ~Shin'ainaru Anata e~", "Re:LieF ～"] {
            let info = parse(title, "Re: LieF ~Shin'ainaru Anata e~", &other, &[]);
            assert_eq!(info.section_label, "", "{title}");
        }
    }

    #[test]
    fn clean_title_matches_python() {
        use crate::engines::clean_title;
        let cases = [
            ("Hapymaher ver1.02 [1280x720]", "Hapymaher"),
            ("Senren*Banka - Steam", "Senren*Banka"),
            ("Game (R18) v1.0.3", "Game"),
            ("Fate/stay night 2.0 Direct3D", "Fate/stay night"),
            (
                "CLANNAD [English HD Edition] May 11 (Sun.)",
                "CLANNAD May 11 (Sun.)",
            ),
            ("Game (Steam Edition) - Chapter 2", "Game - Chapter 2"),
        ];
        for (title, want) in cases {
            assert_eq!(clean_title(title), want, "title: {title}");
        }
    }
}
