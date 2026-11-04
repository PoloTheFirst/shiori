use std::sync::LazyLock;

use fancy_regex::Regex;

use crate::config::game_key;

const EMULATORS: [(&str, &str); 18] = [
    ("ryujinx", "Ryujinx"),
    ("yuzu", "yuzu"),
    ("suyu", "suyu"),
    ("sudachi", "Sudachi"),
    ("citron", "Citron"),
    ("torzu", "Torzu"),
    ("ppsspp", "PPSSPP"),
    ("vita3k", "Vita3K"),
    ("rpcs3", "RPCS3"),
    ("duckstation", "DuckStation"),
    ("pcsx2", "PCSX2"),
    ("dolphin", "Dolphin"),
    ("citra", "Citra"),
    ("lime3ds", "Lime3DS"),
    ("azahar", "Azahar"),
    ("melonds", "melonDS"),
    ("desmume", "DeSmuME"),
    ("retroarch", "RetroArch"),
];

static GAME_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:0100|0004)[0-9A-Fa-f]{12}\b|\b[A-Z]{4}[-_]?\d{5}\b|(?<=\()[A-Z0-9]{6}(?=\))")
        .unwrap()
});
static BRACKETED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^)]*\)|\[[^\]]*\]").unwrap());
static SEPARATORS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*(?:\|\|?|\s[-–—]\s|\s:\s)\s*").unwrap());
static TRAILING_VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s+v?\d+(?:\.\d+)+\S*$").unwrap());
static NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^v?\d[\d.]*\S*$|\b(?:fps|vps|vulkan|opengl|direct3d|d3d1[12]|directx|jit\w*|nvidia|amd|intel|geforce|radeon|64-bit|32-bit)\b|%",
    )
    .unwrap()
});

pub struct EmulatedGame {
    pub emulator: String,
    pub id: String,
    pub name: String,
}

impl EmulatedGame {
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            format!("{} game {}", self.emulator, self.id.to_uppercase())
        } else {
            self.name.clone()
        }
    }
}

pub fn emulator_name(exe: &str) -> Option<&'static str> {
    let stem = game_key(exe);
    EMULATORS
        .iter()
        .find(|(prefix, _)| stem.starts_with(prefix))
        .map(|(_, name)| *name)
}

pub fn game_in_window(exe: &str, title: &str, split_by_title: bool) -> Option<EmulatedGame> {
    let emulator = match emulator_name(exe) {
        Some(name) => name.to_string(),
        None if split_by_title => game_key(exe),
        None => return None,
    };

    let found_id = GAME_ID.find(title).ok().flatten().map(|m| m.as_str().to_string());
    let name = name_from_title(title, &emulator, &game_key(exe));
    let id = found_id
        .map(|id| id.replace(['-', '_'], "").to_lowercase())
        .unwrap_or_else(|| slug(&name));
    if id.is_empty() {
        return None;
    }
    Some(EmulatedGame { emulator, id, name })
}

fn name_from_title(title: &str, emulator: &str, stem: &str) -> String {
    let without_ids = GAME_ID.replace_all(title, "");
    let without_brackets = BRACKETED.replace_all(&without_ids, " ");
    let emulator = emulator.to_lowercase();

    SEPARATORS
        .split(&without_brackets)
        .filter_map(Result::ok)
        .map(|part| part.trim_matches(|c| " -–—:|".contains(c)))
        .map(|part| TRAILING_VERSION.replace(part, "").trim().to_string())
        .find(|part| {
            let lower = part.to_lowercase();
            !part.is_empty()
                && !lower.contains(&emulator)
                && !lower.contains(stem)
                && !NOISE.is_match(part).unwrap_or(false)
        })
        .unwrap_or_default()
}

fn slug(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    slug.split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
        .chars()
        .take(40)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(exe: &str, title: &str) -> Option<(String, String)> {
        game_in_window(exe, title, false).map(|g| (g.id.clone(), g.display_name()))
    }

    #[test]
    fn reads_the_game_from_emulator_titles() {
        assert_eq!(
            game("Ryujinx.exe", "Ryujinx 1.3.272 v1.0.0 (0100859027B4C000) (64-bit)"),
            Some(("0100859027b4c000".into(), "Ryujinx game 0100859027B4C000".into()))
        );
        assert_eq!(
            game(
                "Ryujinx.exe",
                "Ryujinx 1.1.1376 - Famicom Detective Club v1.0.0 (0100D5C00F0A2000) (64-bit)"
            ),
            Some(("0100d5c00f0a2000".into(), "Famicom Detective Club".into()))
        );
        assert_eq!(
            game("PPSSPPWindows64.exe", "PPSSPP v1.17.1 - ULJM05432 : Kanon"),
            Some(("uljm05432".into(), "Kanon".into()))
        );
        assert_eq!(
            game("yuzu.exe", "yuzu 1734 | Steins;Gate | 1.0.0 | Vulkan"),
            Some(("steins_gate".into(), "Steins;Gate".into()))
        );
    }

    #[test]
    fn leaves_other_windows_alone() {
        assert!(game("Ryujinx.exe", "Ryujinx 1.3.272").is_none());
        assert!(game("SiglusEngine.exe", "Sayonara o Oshiete").is_none());
    }

    #[test]
    fn an_emulator_without_a_game_is_not_a_game() {
        let exe = "DeSmuME_0.9.13_x64.exe";
        assert!(emulator_name(exe).is_some());
        assert!(game(exe, "DeSmuME 0.9.13 x64 SSE2").is_none());
        let found = game(exe, "DeSmuME 0.9.13 x64 SSE2 | Tokimeki Memorial Girl's Side").unwrap();
        assert_eq!(found.1, "Tokimeki Memorial Girl's Side");
    }

    #[test]
    fn splits_any_program_by_title_when_asked() {
        let split = game_in_window("myemu.exe", "MyEmu 2.0 | Little Busters!", true).unwrap();
        assert_eq!(split.name, "Little Busters!");
        assert_eq!(split.id, "little_busters");
    }
}
