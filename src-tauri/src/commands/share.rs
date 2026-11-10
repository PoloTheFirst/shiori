use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use base64::Engine as _;
use chrono::{Datelike, NaiveDate};
use serde_json::{json, Map, Value};

use super::library::visible_games;
use super::{blocking, SharedState};
use crate::config::Entry;
use crate::state::Shared;
use crate::{clipboard, images, screenshots};

const WRAPPED_SITE: &str = "https://wrapped.maia.moe/";

#[tauri::command(async)]
pub fn share_games(shared: SharedState) -> Vec<Value> {
    let cfg = shared.config.lock().unwrap();

    visible_games(&cfg)
        .into_iter()
        .map(|(key, entry)| {
            json!({
                "key": key,
                "name": screenshots::display_name(&key, &entry),
                "daily": entry.get("daily").cloned().unwrap_or(json!({})),
                "status": entry.get("status").cloned().unwrap_or(json!("")),
                "finished_at": entry.get("finished_at").cloned().unwrap_or(Value::Null),
                "last_played": entry.get("last_played").cloned().unwrap_or(Value::Null),
                "nsfw": entry.get("cover_nsfw").cloned().unwrap_or(Value::Null),
                "cover": cfg.cached_cover(&entry).map(|p| p.to_string_lossy().to_string()),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn learn_cover_nsfw(
    shared: SharedState<'_>,
    keys: Vec<String>,
) -> Result<Map<String, Value>, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        let mut learned = Map::new();
        for key in keys {
            let entry = shared.config.lock().unwrap().game(&key);
            let source = entry
                .get("cover_source")
                .and_then(Value::as_str)
                .unwrap_or("");
            let vn_id = ["vndb_id", "matched_vndb_id"]
                .iter()
                .find_map(|f| {
                    entry
                        .get(*f)
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty())
                })
                .map(str::to_string);

            let Some(vn_id) = vn_id else { continue };
            if entry.contains_key("cover_nsfw") || source == "local" || source == "url" {
                continue;
            }

            if let Ok(Some(vn)) = shared.vndb.get(&vn_id) {
                let fields = Map::from_iter([("cover_nsfw".into(), json!(vn.nsfw))]);
                shared.config.lock().unwrap().set_game(&key, fields);
                learned.insert(key, json!(vn.nsfw));
            }
        }
        Ok(learned)
    })
    .await
}

#[tauri::command]
pub async fn image_data_url(
    shared: SharedState<'_>,
    path: String,
    width: u32,
    height: u32,
) -> Result<String, String> {
    let thumbs = shared.data_dir.join("cache").join("thumbs");
    blocking(move || images::data_url(&thumbs, Path::new(&path), width, height)).await
}

fn decode_png(data_url: &str) -> Result<Vec<u8>, String> {
    let encoded = data_url
        .split_once(',')
        .map(|(_, data)| data)
        .unwrap_or(data_url);
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn copy_png(data_url: String) -> Result<(), String> {
    blocking(move || clipboard::copy_png(&decode_png(&data_url)?)).await
}

#[tauri::command]
pub async fn save_png(path: String, data_url: String) -> Result<(), String> {
    blocking(move || std::fs::write(&path, decode_png(&data_url)?).map_err(|e| e.to_string())).await
}

fn in_year(stamp: i64, year: i32) -> bool {
    chrono::DateTime::from_timestamp(stamp, 0)
        .is_some_and(|t| t.with_timezone(&chrono::Local).year() == year)
}

fn counter(entry: &Entry, name: &str) -> Vec<(String, i64)> {
    entry
        .get(name)
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_i64().filter(|s| *s > 0).map(|s| (k.clone(), s)))
                .collect()
        })
        .unwrap_or_default()
}

fn longest_streak(days: &BTreeMap<NaiveDate, i64>) -> i64 {
    let mut best = 0;
    let mut run = 0;
    let mut previous: Option<NaiveDate> = None;
    for day in days.keys() {
        run = if previous.and_then(|p| p.succ_opt()) == Some(*day) { run + 1 } else { 1 };
        best = best.max(run);
        previous = Some(*day);
    }
    best
}

fn finished_in(entry: &Entry, year: i32) -> bool {
    entry.get("status").and_then(Value::as_str) == Some("finished")
        && ["finished_at", "last_played"]
            .iter()
            .find_map(|f| entry.get(*f).and_then(Value::as_i64).filter(|t| *t > 0))
            .is_some_and(|t| in_year(t, year))
}

pub fn build_recap(
    year: i32,
    games: &[(String, Entry)],
    shot_times: &[u64],
    lang: &str,
    nsfw: bool,
) -> Value {
    let prefix = format!("{year}-");
    let mut months = [0i64; 12];
    let mut hours = [0i64; 24];
    let mut days: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    let mut read: Vec<(i64, Value)> = Vec::new();
    let mut finished = 0;
    let mut first: Option<(NaiveDate, i64, Value)> = None;

    for (key, entry) in games {
        let mut seconds = 0;
        let mut started: Option<(NaiveDate, i64)> = None;
        for (day, s) in counter(entry, "daily") {
            let Ok(date) = NaiveDate::parse_from_str(&day, "%Y-%m-%d") else {
                continue;
            };
            if date.year() == year {
                seconds += s;
                months[date.month0() as usize] += s;
                *days.entry(date).or_default() += s;
                if started.is_none_or(|(d, _)| date < d) {
                    started = Some((date, s));
                }
            }
        }
        for (slot, s) in counter(entry, "hourly") {
            if let Some(hour) = slot
                .strip_prefix(&prefix)
                .and_then(|h| h.parse::<usize>().ok())
                .filter(|h| *h < 24)
            {
                hours[hour] += s;
            }
        }

        let done = finished_in(entry, year);
        if done {
            finished += 1;
        }
        if seconds < 60 {
            continue;
        }
        let title: String = screenshots::display_name(key, entry).chars().take(60).collect();
        let mut vn = json!({ "t": title, "s": seconds });
        if let Some(id) = ["vndb_id", "matched_vndb_id"]
            .iter()
            .filter_map(|f| entry.get(*f).and_then(Value::as_str))
            .find(|id| !id.is_empty())
        {
            vn["id"] = json!(id);
        }
        if done {
            vn["f"] = json!(1);
        }
        if let Some(vote) = entry
            .get("vndb_vote")
            .and_then(Value::as_i64)
            .filter(|v| (10..=100).contains(v))
        {
            vn["u"] = json!(vote);
        }
        if let Some((date, s)) = started {
            let earlier = first
                .as_ref()
                .is_none_or(|(d, best, _)| date < *d || (date == *d && s > *best));
            if earlier {
                let mut pick = json!({ "t": vn["t"] });
                if let Some(id) = vn.get("id") {
                    pick["id"] = id.clone();
                }
                first = Some((date, s, pick));
            }
        }
        read.push((seconds, vn));
    }

    read.sort_by_key(|(seconds, _)| std::cmp::Reverse(*seconds));
    let mut recap = json!({
        "v": 1,
        "y": year,
        "lang": lang,
        "t": days.values().sum::<i64>(),
        "n": read.len(),
        "f": finished,
        "dr": days.len(),
        "st": longest_streak(&days),
        "m": months,
        "top": read.into_iter().take(5).map(|(_, vn)| vn).collect::<Vec<_>>(),
    });
    if hours.iter().any(|h| *h > 0) {
        recap["hr"] = json!(hours);
    }
    if let Some((day, s)) = days
        .iter()
        .max_by_key(|(day, s)| (**s, std::cmp::Reverse(**day)))
    {
        recap["bd"] = json!([day.ordinal(), s]);
    }
    if !days.is_empty() {
        let mut week = [0i64; 7];
        for (day, s) in &days {
            week[day.weekday().num_days_from_monday() as usize] += s;
        }
        recap["wd"] = json!(week);
    }
    if let Some((_, _, vn)) = first {
        recap["fv"] = vn;
    }
    let shots = shot_times.iter().filter(|t| in_year(**t as i64, year)).count();
    if shots > 0 {
        recap["sh"] = json!(shots);
    }
    if nsfw {
        recap["x"] = json!(1);
    }
    recap
}

fn recap_of(shared: &Shared, year: i32) -> Value {
    let mut cfg = shared.config.lock().unwrap();
    let games = visible_games(&cfg);
    let shots: Vec<u64> = screenshots::games_with_shots(&mut cfg)
        .into_iter()
        .flat_map(|game| game.times)
        .collect();
    let lang = crate::lang::resolve(&cfg.str("language"));
    build_recap(year, &games, &shots, lang, cfg.bool("allow_nsfw_covers"))
}

#[tauri::command(async)]
pub fn year_recap(shared: SharedState, year: i32) -> Value {
    let recap = recap_of(&shared, year);
    json!({ "url": format!("{WRAPPED_SITE}?d={}", pack(&recap)), "seconds": recap["t"] })
}

fn short_link(packed: &str) -> Option<String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("ShioriRPC/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .ok()?;
    let answer: Value = client
        .post(format!("{WRAPPED_SITE}api/r"))
        .json(&json!({ "d": packed }))
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .ok()?;
    answer["url"]
        .as_str()
        .filter(|url| url.starts_with(WRAPPED_SITE))
        .map(str::to_string)
}

#[tauri::command]
pub async fn recap_link(shared: SharedState<'_>, year: i32) -> Result<String, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        let packed = pack(&recap_of(&shared, year));
        Ok(short_link(&packed).unwrap_or_else(|| format!("{WRAPPED_SITE}?d={packed}")))
    })
    .await
}

fn minutes(value: &Value) -> Value {
    json!((value.as_i64().unwrap_or(0) + 30) / 60)
}

fn minute_list(value: &Value) -> Value {
    Value::Array(
        value
            .as_array()
            .map(|list| list.iter().map(minutes).collect())
            .unwrap_or_default(),
    )
}

fn vn_ref(vn: &Value) -> Value {
    vn.get("id")
        .and_then(Value::as_str)
        .and_then(|id| id.strip_prefix('v'))
        .and_then(|n| n.parse::<u64>().ok())
        .map(|n| json!(n))
        .unwrap_or_else(|| vn["t"].clone())
}

pub fn compact(recap: &Value) -> Value {
    let top: Vec<Value> = recap["top"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|vn| {
                    json!([
                        vn_ref(vn),
                        minutes(&vn["s"]),
                        vn["f"].as_i64().unwrap_or(0),
                        vn["u"].as_i64().unwrap_or(0)
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    let big_day = match recap.get("bd").and_then(Value::as_array) {
        Some(day) if day.len() == 2 => json!([day[0], minutes(&day[1])]),
        _ => json!([]),
    };
    json!([
        2,
        recap["y"],
        recap["lang"],
        minutes(&recap["t"]),
        recap["n"],
        recap["f"],
        recap["dr"],
        recap["st"],
        minute_list(&recap["m"]),
        minute_list(&recap["hr"]),
        recap["sh"].as_i64().unwrap_or(0),
        recap["x"].as_i64().unwrap_or(0),
        big_day,
        minute_list(&recap["wd"]),
        recap.get("fv").map(vn_ref).unwrap_or(json!(0)),
        top
    ])
}

pub fn pack(recap: &Value) -> String {
    use std::io::Write as _;
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    let _ = encoder.write_all(compact(recap).to_string().as_bytes());
    let bytes = encoder.finish().unwrap_or_default();
    format!(
        "2.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn game(fields: Value) -> Entry {
        fields.as_object().unwrap().clone()
    }

    #[test]
    fn recap_counts_only_the_asked_year() {
        let finished_2026 = chrono::Local
            .with_ymd_and_hms(2026, 6, 1, 12, 0, 0)
            .unwrap()
            .timestamp();
        let games = vec![
            (
                "a.exe".to_string(),
                game(json!({
                    "title": "Senren * Banka",
                    "vndb_id": "v19073",
                    "status": "finished",
                    "finished_at": finished_2026,
                    "daily": { "2025-12-31": 999, "2026-03-01": 3600, "2026-03-02": 1800, "2026-03-04": 600 },
                    "hourly": { "2025-23": 999, "2026-22": 6000 },
                })),
            ),
            (
                "b.exe".to_string(),
                game(json!({ "name": "Other", "daily": { "2026-03-02": 7200 } })),
            ),
            ("c.exe".to_string(), game(json!({ "name": "Old", "daily": { "2025-01-01": 9000 } }))),
        ];

        let recap = build_recap(2026, &games, &[], "fr", false);

        assert_eq!(recap["t"], 13200);
        assert_eq!(recap["n"], 2);
        assert_eq!(recap["f"], 1);
        assert_eq!(recap["dr"], 3);
        assert_eq!(recap["st"], 2);
        assert_eq!(recap["m"][2], 13200);
        assert_eq!(recap["hr"][22], 6000);
        assert_eq!(recap["top"][0]["t"], "Other");
        assert_eq!(recap["top"][1]["id"], "v19073");
        assert_eq!(recap["top"][1]["f"], 1);
        assert!(recap.get("sh").is_none());
        assert!(recap.get("x").is_none());
        assert_eq!(recap["bd"], json!([61, 9000]));
        assert_eq!(recap["wd"][0], 9000);
        assert_eq!(recap["fv"]["id"], "v19073");
    }

    #[test]
    fn packed_recap_is_short_and_round_trips() {
        use std::io::Read as _;
        let recap = json!({
            "v": 1, "y": 2026, "lang": "fr", "t": 687600, "n": 18, "f": 7, "dr": 164, "st": 21,
            "m": [28800, 36000, 43200, 50400, 57600, 79200, 93600, 122400, 54000, 50400, 39600, 32400],
            "hr": [63440, 60320, 50960, 34320, 12480, 4160, 0, 0, 0, 1040, 2080, 3120, 5200, 6240, 8320, 10400, 12480, 15600, 20800, 27040, 32240, 39520, 48880, 57200],
            "sh": 92, "bd": [226, 33120], "wd": [80000, 70000, 60000, 90000, 100000, 150000, 137600],
            "fv": { "id": "v19073", "t": "Senren＊Banka" },
            "top": [
                { "id": "v18852", "t": "Amatsutsumi", "s": 127113, "f": 1, "u": 90 },
                { "id": "v19073", "t": "Senren＊Banka", "s": 126685, "f": 1 },
                { "id": "v3770", "t": "Aiyoku no Eustia", "s": 117357, "f": 1, "u": 100 },
                { "id": "v10957", "t": "Hapymaher", "s": 77992, "f": 1 },
                { "t": "My own game", "s": 51630 }
            ]
        });
        let old = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(recap.to_string());
        let packed = pack(&recap);
        assert!(packed.len() * 3 < old.len(), "{} vs {}", packed.len(), old.len());

        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(packed.strip_prefix("2.").unwrap())
            .unwrap();
        let mut text = String::new();
        flate2::read::DeflateDecoder::new(&bytes[..])
            .read_to_string(&mut text)
            .unwrap();
        let data: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(data[0], 2);
        assert_eq!(data[3], 11460);
        assert_eq!(data[12], json!([226, 552]));
        assert_eq!(data[14], 19073);
        assert_eq!(data[15][0], json!([18852, 2119, 1, 90]));
        assert_eq!(data[15][4], json!(["My own game", 861, 0, 0]));
        println!("old {} new {}", old.len(), packed.len());
    }
}
