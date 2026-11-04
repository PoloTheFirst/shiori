use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::LazyLock;

use fancy_regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::Shared;
use crate::vndb::{is_nsfw, read_cached};

const CACHE_DAYS: u64 = 1;
const SEED_LIMIT: usize = 200;
const TASTE_SHORTLIST: usize = 60;
const TASTE_SIZE: usize = 25;
const TASTE_SHOWN: usize = 8;
const SEARCHED_TAGS: usize = 10;
const TAG_SEARCH_PAGES: u32 = 2;
const SEARCHED_STUDIOS: usize = 5;
const SEARCHED_STAFF: usize = 6;
const RESULTS_PER_SEARCH: u32 = 100;
const MIN_VOTES: u32 = 100;
const KEPT_RESULTS: usize = 150;
const VNS_ON_VNDB: f64 = 60000.0;
const UNKNOWN_TAG_COUNT: f64 = 3000.0;
const MIN_TAG_LEVEL: f64 = 1.5;
const RANK_DAMPING: f64 = 60.0;
const TASTE_WEIGHT: f64 = 3.0;
const STUDIO_WEIGHT: f64 = 1.0;
const STAFF_WEIGHT: f64 = 1.0;
const VOICE_WEIGHT: f64 = 0.5;
const CREATOR_PRIOR: f64 = 3.0;
const TAG_FIELDS: &str = "tags.id,tags.name,tags.rating,tags.spoiler,tags.category";
const CREATOR_FIELDS: &str = "developers.id,developers.name,staff.id,staff.name,staff.role,va.staff.id";
const WRITING_ROLES: [&str; 2] = ["scenario", "director"];

static NOT_A_TASTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)heroine|protagonist|hero\b|loli|shota|breast|hair|twin tail|zettai|student|uniform|glasses|\bage\b|\bages\b|teen|adult|under the same roof|route|branching|nukige|sexual|\bsex\b|nudity|voiced|\badv\b|\bnvl\b|brothel|brother/sister|sister|incest|rape|prostitut",
    )
    .unwrap()
});

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Signals {
    pub tags: f64,
    pub studios: f64,
    pub staff: f64,
    pub voices: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Recommendation {
    pub id: String,
    pub title: String,
    pub description: String,
    pub signals: Signals,
    pub year: String,
    pub rating: f64,
    pub votes: i64,
    pub minutes: i64,
    pub language: String,
    pub developer: String,
    pub image_url: String,
    pub nsfw: bool,
    pub because: String,
    pub matched: Vec<String>,
    pub credits: Vec<String>,
    pub score: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Recommendations {
    pub partial: bool,
    pub liked: usize,
    pub taste: Vec<String>,
    pub vns: Vec<Recommendation>,
}

#[derive(Debug, Clone)]
struct Taste {
    id: String,
    name: String,
    score: f64,
    count: u32,
    value: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Creator {
    Studio,
    Staff,
    Voice,
}

#[derive(Default, Clone, Copy)]
struct Seed {
    vote: i64,
    weight: f64,
}

struct ReadVn {
    weight: f64,
    feeling: f64,
    title: String,
    tags: Vec<Value>,
    raw: Value,
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or("").to_string()
}

fn seed_weight(vote: i64, seconds: i64, status: &str) -> f64 {
    if vote >= 60 {
        return (vote - 50) as f64 / 50.0;
    }
    if vote > 0 || status == "dropped" {
        return 0.0;
    }
    let hours = seconds as f64 / 3600.0;
    if hours >= 3.0 {
        (0.3 + hours / 60.0).min(0.8)
    } else {
        0.0
    }
}

fn feeling(seed: Seed, average_vote: f64) -> f64 {
    if seed.vote > 0 {
        ((seed.vote as f64 - average_vote) / 15.0).clamp(-2.0, 2.0)
    } else {
        seed.weight
    }
}

fn tag_counts_as_taste(tag: &Value) -> bool {
    tag["category"] == "cont"
        && tag["spoiler"].as_f64() == Some(0.0)
        && tag["rating"].as_f64().unwrap_or(0.0) >= MIN_TAG_LEVEL
        && !NOT_A_TASTE.is_match(tag["name"].as_str().unwrap_or("")).unwrap_or(true)
}

fn taste_tags(vn: &Value) -> Vec<Value> {
    vn["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|tag| tag_counts_as_taste(tag))
        .cloned()
        .collect()
}

fn creators_of(vn: &Value, kind: Creator) -> Vec<(String, String)> {
    let list = |field: &str| vn[field].as_array().cloned().unwrap_or_default();
    let mut found: Vec<(String, String)> = match kind {
        Creator::Studio => list("developers")
            .iter()
            .map(|d| (text(&d["id"]), text(&d["name"])))
            .collect(),
        Creator::Staff => list("staff")
            .iter()
            .filter(|s| WRITING_ROLES.contains(&s["role"].as_str().unwrap_or("")))
            .map(|s| (text(&s["id"]), text(&s["name"])))
            .collect(),
        Creator::Voice => list("va")
            .iter()
            .map(|v| (text(&v["staff"]["id"]), String::new()))
            .collect(),
    };
    found.retain(|(id, _)| !id.is_empty());
    found.sort();
    found.dedup_by(|a, b| a.0 == b.0);
    found
}

fn shortlist_taste(read: &[ReadVn]) -> Vec<Taste> {
    let mut tastes: HashMap<String, Taste> = HashMap::new();
    for vn in read.iter().filter(|vn| vn.weight > 0.0) {
        for tag in &vn.tags {
            let id = text(&tag["id"]);
            let taste = tastes.entry(id.clone()).or_insert_with(|| Taste {
                id,
                name: text(&tag["name"]),
                score: 0.0,
                count: 0,
                value: 0.0,
            });
            taste.score += vn.weight * tag["rating"].as_f64().unwrap_or(0.0) / 3.0;
            taste.count += 1;
        }
    }

    let mut list: Vec<Taste> = tastes.into_values().filter(|t| t.count >= 2).collect();
    list.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    list.truncate(TASTE_SHORTLIST);
    list
}

fn weigh_by_rarity(tastes: &mut [Taste], counts: &HashMap<String, f64>) {
    for taste in tastes.iter_mut() {
        let count = counts.get(&taste.id).copied().unwrap_or(UNKNOWN_TAG_COUNT).max(1.0);
        taste.value = taste.score * (VNS_ON_VNDB / count).ln().max(0.0);
    }
    tastes.sort_by(|a, b| b.value.total_cmp(&a.value).then_with(|| a.id.cmp(&b.id)));
}

fn creator_affinity(read: &[ReadVn], kind: Creator) -> HashMap<String, f64> {
    let mut totals: HashMap<String, (f64, f64)> = HashMap::new();
    for vn in read {
        for (id, _) in creators_of(&vn.raw, kind) {
            let total = totals.entry(id).or_insert((0.0, 0.0));
            total.0 += vn.feeling;
            total.1 += 1.0;
        }
    }
    totals
        .into_iter()
        .map(|(id, (sum, count))| {
            (id, sum / (count + CREATOR_PRIOR) * (count / CREATOR_PRIOR).min(1.0))
        })
        .collect()
}

fn creator_score(vn: &Value, kind: Creator, affinity: &HashMap<String, f64>) -> f64 {
    let mut scores: Vec<f64> = creators_of(vn, kind)
        .iter()
        .map(|(id, _)| affinity.get(id).copied().unwrap_or(0.0).max(0.0))
        .collect();
    scores.sort_by(|a, b| b.total_cmp(a));
    scores.iter().take(3).sum()
}

fn favorite_creators(read: &[ReadVn], kind: Creator, limit: usize) -> Vec<String> {
    let mut totals: HashMap<String, f64> = HashMap::new();
    for vn in read.iter().filter(|vn| vn.weight > 0.0 && vn.feeling > 0.0) {
        for (id, _) in creators_of(&vn.raw, kind) {
            *totals.entry(id).or_insert(0.0) += vn.feeling;
        }
    }
    let mut list: Vec<(String, f64)> = totals.into_iter().collect();
    list.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    list.into_iter().take(limit).map(|(id, _)| id).collect()
}

fn closest_read(matched: &HashSet<String>, read: &[ReadVn]) -> String {
    let mut best = (0.0, String::new());
    for vn in read.iter().filter(|vn| vn.weight > 0.0) {
        let shared_tags = vn
            .tags
            .iter()
            .filter(|tag| matched.contains(tag["id"].as_str().unwrap_or("")))
            .count();
        let shared_creators = [Creator::Studio, Creator::Staff]
            .iter()
            .flat_map(|kind| creators_of(&vn.raw, *kind))
            .filter(|(id, _)| matched.contains(id))
            .count();
        let shared = (shared_tags + shared_creators * 2) as f64 * vn.weight;
        if shared > best.0 {
            best = (shared, vn.title.clone());
        }
    }
    best.1
}

fn place_scores(scores: &[f64], weight: f64, totals: &mut [f64]) {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then_with(|| a.cmp(&b)));
    for (place, index) in order.into_iter().enumerate() {
        if scores[index] > 0.0 {
            totals[index] += weight / (RANK_DAMPING + place as f64);
        }
    }
}

fn rank(candidates: &[Value], tastes: &[Taste], read: &[ReadVn]) -> Vec<Recommendation> {
    let by_id: HashMap<&str, &Taste> = tastes.iter().map(|t| (t.id.as_str(), t)).collect();
    let total: f64 = tastes.iter().map(|t| t.value).sum();
    if total <= 0.0 {
        return vec![];
    }

    let parts_of = |vn: &Value| -> Vec<(String, String, f64)> {
        let mut parts: Vec<(String, String, f64)> = taste_tags(vn)
            .iter()
            .filter_map(|tag| {
                let id = text(&tag["id"]);
                let taste = by_id.get(id.as_str())?;
                let part = taste.value * tag["rating"].as_f64().unwrap_or(0.0) / 3.0;
                Some((id, taste.name.clone(), part))
            })
            .collect();
        parts.sort_by(|a, b| b.2.total_cmp(&a.2));
        parts
    };

    let mut totals = vec![0.0; candidates.len()];
    let taste_scores: Vec<f64> = candidates
        .iter()
        .map(|vn| {
            let parts = parts_of(vn);
            if parts.is_empty() {
                return 0.0;
            }
            let fit = parts.iter().map(|(_, _, part)| part).sum::<f64>() / total;
            fit * 0.75 + vn["rating"].as_f64().unwrap_or(0.0) / 100.0 * 0.25
        })
        .collect();
    place_scores(&taste_scores, TASTE_WEIGHT, &mut totals);

    let mut liked_creators: HashMap<String, f64> = HashMap::new();
    let mut creator_scores = |kind: Creator, weight: f64| -> Vec<f64> {
        let affinity = creator_affinity(read, kind);
        let scores: Vec<f64> = candidates
            .iter()
            .map(|vn| creator_score(vn, kind, &affinity))
            .collect();
        place_scores(&scores, weight, &mut totals);
        if kind != Creator::Voice {
            liked_creators.extend(affinity.into_iter().filter(|(_, value)| *value > 0.0));
        }
        scores
    };
    let studio_scores = creator_scores(Creator::Studio, STUDIO_WEIGHT);
    let staff_scores = creator_scores(Creator::Staff, STAFF_WEIGHT);
    let voice_scores = creator_scores(Creator::Voice, VOICE_WEIGHT);

    let mut out: Vec<Recommendation> = candidates
        .iter()
        .enumerate()
        .filter(|(index, _)| totals[*index] > 0.0)
        .map(|(index, vn)| {
            let score = totals[index];
            let parts = parts_of(vn);
            let credits: Vec<(String, String)> = [Creator::Staff, Creator::Studio]
                .iter()
                .flat_map(|kind| creators_of(vn, *kind))
                .filter(|(id, _)| liked_creators.contains_key(id))
                .collect();
            let shared: HashSet<String> = parts
                .iter()
                .map(|(id, _, _)| id.clone())
                .chain(credits.iter().map(|(id, _)| id.clone()))
                .collect();
            Recommendation {
                id: text(&vn["id"]),
                title: text(&vn["title"]),
                description: text(&vn["description"]),
                signals: Signals {
                    tags: taste_scores[index],
                    studios: studio_scores[index],
                    staff: staff_scores[index],
                    voices: voice_scores[index],
                },
                year: text(&vn["released"]).chars().take(4).collect(),
                rating: vn["rating"].as_f64().unwrap_or(0.0) / 10.0,
                votes: vn["votecount"].as_i64().unwrap_or(0),
                minutes: vn["length_minutes"].as_i64().unwrap_or(0),
                language: text(&vn["olang"]),
                developer: text(&vn["developers"][0]["name"]),
                image_url: text(&vn["image"]["url"]),
                nsfw: is_nsfw(&vn["image"]),
                because: closest_read(&shared, read),
                matched: parts.into_iter().map(|(_, name, _)| name).collect(),
                credits: credits.into_iter().map(|(_, name)| name).take(2).collect(),
                score,
            }
        })
        .collect();

    out.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    out.truncate(KEPT_RESULTS);
    out
}

fn results(data: &Value) -> Vec<Value> {
    data["results"].as_array().cloned().unwrap_or_default()
}

fn either(mut filters: Vec<Value>) -> Value {
    if filters.len() == 1 {
        return filters.remove(0);
    }
    let mut all = vec![json!("or")];
    all.append(&mut filters);
    Value::Array(all)
}

fn any_of(field: &str, ids: &[String]) -> Value {
    either(ids.iter().map(|id| json!([field, "=", id])).collect())
}

fn creator_filter(field: &str, ids: &[String]) -> Value {
    either(ids.iter().map(|id| json!([field, "=", ["id", "=", id]])).collect())
}

struct Library {
    token: String,
    seeds: HashMap<String, Seed>,
    seen: HashSet<String>,
}

fn read_library(shared: &Shared) -> Library {
    let cfg = shared.config.lock().unwrap();
    let mut library = Library {
        token: cfg.str("vndb_token").trim().to_string(),
        seeds: HashMap::new(),
        seen: HashSet::new(),
    };
    for entry in cfg.all_games().values() {
        let id = ["vndb_id", "matched_vndb_id"]
            .iter()
            .filter_map(|field| entry.get(*field).and_then(Value::as_str))
            .find(|id| !id.is_empty())
            .unwrap_or("");
        if id.is_empty() {
            continue;
        }
        library.seen.insert(id.to_string());
        let vote = entry.get("vndb_vote").and_then(Value::as_i64).unwrap_or(0);
        let weight = seed_weight(
            vote,
            entry.get("playtime_seconds").and_then(Value::as_i64).unwrap_or(0),
            entry.get("status").and_then(Value::as_str).unwrap_or(""),
        );
        if vote > 0 || weight > 0.0 {
            library.seeds.insert(id.to_string(), Seed { vote, weight });
        }
    }
    library
}

fn report(shared: &Shared, stage: &str, percent: u32, done: usize, total: usize) {
    shared.emit(
        "reco-progress",
        json!({ "stage": stage, "percent": percent, "done": done, "total": total }),
    );
}

fn compute(shared: &Shared, mut library: Library) -> Result<Recommendations, String> {
    let vndb = &shared.vndb;

    if !library.token.is_empty() {
        report(shared, "list", 5, 0, 0);
        for (id, vote) in vndb.list_votes(&library.token).unwrap_or_default() {
            library.seen.insert(id.clone());
            if vote > 0 {
                let seed = library.seeds.entry(id).or_default();
                seed.vote = vote;
                seed.weight = seed.weight.max(seed_weight(vote, 0, ""));
            }
        }
    }

    let votes: Vec<f64> = library
        .seeds
        .values()
        .filter(|seed| seed.vote > 0)
        .map(|seed| seed.vote as f64)
        .collect();
    let average_vote = votes.iter().sum::<f64>() / (votes.len().max(1) as f64);

    let mut seeds: Vec<(String, Seed)> = library.seeds.into_iter().collect();
    seeds.sort_by(|a, b| b.1.weight.total_cmp(&a.1.weight).then_with(|| a.0.cmp(&b.0)));
    seeds.truncate(SEED_LIMIT);
    let by_id: HashMap<String, Seed> = seeds.iter().cloned().collect();

    report(shared, "favorites", 15, 0, 0);
    let mut read = Vec::new();
    for batch in seeds.chunks(100) {
        let ids: Vec<String> = batch.iter().map(|(id, _)| id.clone()).collect();
        let body = json!({
            "filters": any_of("id", &ids),
            "fields": format!("title,{TAG_FIELDS},{CREATOR_FIELDS}"),
            "results": 100,
        });
        for vn in results(&vndb.post("/vn", &body)?) {
            let seed = by_id.get(vn["id"].as_str().unwrap_or("")).copied().unwrap_or_default();
            read.push(ReadVn {
                weight: seed.weight,
                feeling: feeling(seed, average_vote),
                title: text(&vn["title"]),
                tags: taste_tags(&vn),
                raw: vn,
            });
        }
    }
    let liked = read.iter().filter(|vn| vn.weight > 0.0).count();

    report(shared, "taste", 25, 0, 0);
    let mut tastes = shortlist_taste(&read);
    let mut counts = HashMap::new();
    for batch in tastes.chunks(50) {
        let ids: Vec<String> = batch.iter().map(|t| t.id.clone()).collect();
        let body = json!({
            "filters": any_of("id", &ids),
            "fields": "vn_count",
            "results": 50,
        });
        for tag in results(&vndb.post("/tag", &body)?) {
            counts.insert(text(&tag["id"]), tag["vn_count"].as_f64().unwrap_or(UNKNOWN_TAG_COUNT));
        }
    }
    weigh_by_rarity(&mut tastes, &counts);
    tastes.truncate(TASTE_SIZE);

    let fields = format!(
        "title,image.url,image.sexual,image.violence,rating,votecount,released,length_minutes,olang,{TAG_FIELDS},{CREATOR_FIELDS}"
    );
    let search = |filters: Value, page: u32| {
        json!({
            "filters": filters,
            "fields": fields,
            "sort": "rating",
            "reverse": true,
            "results": RESULTS_PER_SEARCH,
            "page": page,
        })
    };
    let mut searches: Vec<Value> = Vec::new();
    let tag_filters: Vec<Value> = tastes
        .iter()
        .take(SEARCHED_TAGS)
        .map(|taste| json!(["tag", "=", [taste.id, 0, MIN_TAG_LEVEL]]))
        .collect();
    if !tag_filters.is_empty() {
        let filters = json!(["and", either(tag_filters), ["votecount", ">=", MIN_VOTES]]);
        searches.extend((1..=TAG_SEARCH_PAGES).map(|page| search(filters.clone(), page)));
    }
    let studios = favorite_creators(&read, Creator::Studio, SEARCHED_STUDIOS);
    if !studios.is_empty() {
        searches.push(search(creator_filter("developer", &studios), 1));
    }
    let staff = favorite_creators(&read, Creator::Staff, SEARCHED_STAFF);
    if !staff.is_empty() {
        searches.push(search(creator_filter("staff", &staff), 1));
    }

    let mut candidates: HashMap<String, Value> = HashMap::new();
    let mut partial = false;
    for (index, body) in searches.iter().enumerate() {
        report(shared, "search", 35 + (55 * index / searches.len()) as u32, index + 1, searches.len());
        let data = match vndb.post("/vn", body) {
            Ok(data) => data,
            Err(_) if !candidates.is_empty() => {
                partial = true;
                break;
            }
            Err(error) => return Err(error),
        };
        for vn in results(&data) {
            let id = text(&vn["id"]);
            if !library.seen.contains(&id) {
                candidates.insert(id, vn);
            }
        }
    }

    report(shared, "rank", 95, 0, 0);
    let mut candidates: Vec<Value> = candidates.into_values().collect();
    candidates.sort_by_key(|vn| text(&vn["id"]));
    let mut vns = rank(&candidates, &tastes, &read);
    add_descriptions(shared, &mut vns);
    Ok(Recommendations {
        partial,
        liked,
        taste: tastes.iter().take(TASTE_SHOWN).map(|t| t.name.clone()).collect(),
        vns,
    })
}

fn add_descriptions(shared: &Shared, vns: &mut [Recommendation]) {
    let ids: Vec<String> = vns.iter().take(100).map(|vn| vn.id.clone()).collect();
    if ids.is_empty() {
        return;
    }
    let body = json!({ "filters": any_of("id", &ids), "fields": "description", "results": 100 });
    let Ok(data) = shared.vndb.post("/vn", &body) else {
        return;
    };
    let descriptions: HashMap<String, String> = results(&data)
        .iter()
        .map(|vn| (text(&vn["id"]), text(&vn["description"])))
        .collect();
    for vn in vns {
        if let Some(description) = descriptions.get(&vn.id) {
            vn.description = description.clone();
        }
    }
}

pub fn recommendations(shared: &Shared, refresh: bool) -> Result<Recommendations, String> {
    let cache = shared.data_dir.join("cache").join("for_you_v2.json");
    let library = read_library(shared);
    let seen = library.seen.clone();

    let cached = (!refresh)
        .then(|| read_cached(&cache, CACHE_DAYS))
        .flatten()
        .and_then(|raw| serde_json::from_value::<Recommendations>(raw).ok());

    let mut found = match cached {
        Some(found) => found,
        None => {
            let found = compute(shared, library)?;
            if !found.partial {
                if let Ok(raw) = serde_json::to_string(&found) {
                    let _ = fs::create_dir_all(shared.data_dir.join("cache"));
                    let _ = fs::write(&cache, raw);
                }
            }
            found
        }
    };
    found.vns.retain(|vn| !seen.contains(&vn.id));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(id: &str, name: &str, rating: f64) -> Value {
        json!({ "id": id, "name": name, "rating": rating, "spoiler": 0, "category": "cont" })
    }

    fn taste(id: &str, name: &str, value: f64) -> Taste {
        Taste { id: id.into(), name: name.into(), score: value, count: 2, value }
    }

    fn read_vn(title: &str, weight: f64, feeling: f64, raw: Value) -> ReadVn {
        ReadVn { weight, feeling, title: title.into(), tags: taste_tags(&raw), raw }
    }

    #[test]
    fn groups_searches_into_one_filter() {
        let ids = ["p1".to_string(), "p2".to_string()];
        assert_eq!(
            creator_filter("developer", &ids),
            json!(["or", ["developer", "=", ["id", "=", "p1"]], ["developer", "=", ["id", "=", "p2"]]])
        );
        assert_eq!(any_of("id", &ids[..1]), json!(["id", "=", "p1"]));
    }

    #[test]
    fn weighs_what_the_reader_liked() {
        assert_eq!(seed_weight(100, 0, ""), 1.0);
        assert_eq!(seed_weight(70, 0, ""), 0.4);
        assert_eq!(seed_weight(40, 360000, "finished"), 0.0);
        assert_eq!(seed_weight(0, 360000, "dropped"), 0.0);
        assert_eq!(seed_weight(0, 3600, "finished"), 0.0);
        assert_eq!(seed_weight(0, 360000, "finished"), 0.8);
    }

    #[test]
    fn feeling_compares_a_vote_to_the_readers_average() {
        assert_eq!(feeling(Seed { vote: 85, weight: 0.7 }, 70.0), 1.0);
        assert_eq!(feeling(Seed { vote: 40, weight: 0.0 }, 70.0), -2.0);
        assert_eq!(feeling(Seed { vote: 0, weight: 0.5 }, 70.0), 0.5);
    }

    #[test]
    fn only_stories_and_themes_count_as_taste() {
        assert!(tag_counts_as_taste(&tag("g1", "Time Travel", 2.0)));
        assert!(tag_counts_as_taste(&tag("g2", "High School", 2.0)));
        for name in ["Male Protagonist", "Loli Heroine", "Twin Tail Heroine", "High School Student Heroine", "Brother/Sister Romance"] {
            assert!(!tag_counts_as_taste(&tag("g3", name, 3.0)), "{name}");
        }
        assert!(!tag_counts_as_taste(&tag("g1", "Time Travel", 1.0)));
        let mut spoiler = tag("g1", "Time Travel", 3.0);
        spoiler["spoiler"] = json!(2);
        assert!(!tag_counts_as_taste(&spoiler));
        let mut technical = tag("g1", "Time Travel", 3.0);
        technical["category"] = json!("tech");
        assert!(!tag_counts_as_taste(&technical));
    }

    #[test]
    fn rare_tags_outweigh_common_ones() {
        let mut tastes = vec![taste("common", "Comedy", 1.0), taste("rare", "Time Loop", 1.0)];
        let counts = HashMap::from([("common".to_string(), 30000.0), ("rare".to_string(), 300.0)]);
        weigh_by_rarity(&mut tastes, &counts);
        assert_eq!(tastes[0].id, "rare");
    }

    #[test]
    fn creators_only_count_for_their_real_roles_once() {
        let vn = json!({
            "developers": [{ "id": "p1", "name": "Key" }],
            "staff": [
                { "id": "s1", "name": "Writer", "role": "scenario" },
                { "id": "s1", "name": "Writer", "role": "director" },
                { "id": "s2", "name": "Artist", "role": "art" },
            ],
            "va": [{ "staff": { "id": "s3" } }, { "staff": { "id": "s3" } }],
        });
        assert_eq!(creators_of(&vn, Creator::Studio), [("p1".to_string(), "Key".to_string())]);
        assert_eq!(creators_of(&vn, Creator::Staff), [("s1".to_string(), "Writer".to_string())]);
        assert_eq!(creators_of(&vn, Creator::Voice).len(), 1);
    }

    #[test]
    fn creators_need_several_good_reads_to_matter() {
        let by = |id: &str| json!({ "developers": [{ "id": id, "name": id }] });
        let read = vec![
            read_vn("A", 1.0, 1.0, by("many")),
            read_vn("B", 1.0, 1.0, by("many")),
            read_vn("C", 1.0, 1.0, by("many")),
            read_vn("D", 1.0, 1.0, by("once")),
            read_vn("E", 0.0, -1.0, by("disliked")),
        ];
        let affinity = creator_affinity(&read, Creator::Studio);
        assert!(affinity["many"] > affinity["once"]);
        assert!(affinity["once"] > 0.0);
        assert!(affinity["disliked"] < 0.0);
        assert_eq!(creator_score(&by("disliked"), Creator::Studio, &affinity), 0.0);
        assert_eq!(favorite_creators(&read, Creator::Studio, 1), ["many"]);
    }

    #[test]
    fn the_best_loved_creators_are_searched_not_the_most_read() {
        let by = |id: &str| json!({ "developers": [{ "id": id, "name": id }] });
        let read = vec![
            read_vn("Masterpiece", 1.0, 2.0, by("august")),
            read_vn("Fine 1", 0.4, 0.5, by("prolific")),
            read_vn("Fine 2", 0.4, 0.5, by("prolific")),
            read_vn("Fine 3", 0.4, 0.5, by("prolific")),
            read_vn("Below average 1", 0.2, -0.5, by("neko")),
            read_vn("Below average 2", 0.2, -0.5, by("neko")),
            read_vn("Below average 3", 0.2, -0.5, by("neko")),
            read_vn("Below average 4", 0.2, -0.5, by("neko")),
        ];
        assert_eq!(favorite_creators(&read, Creator::Studio, 5), ["august", "prolific"]);
    }

    #[test]
    fn ranks_by_fit_and_says_why() {
        let tastes = vec![taste("g1", "Time Loop", 3.0), taste("g2", "Comedy", 1.0)];
        let read = vec![
            read_vn("Loved one", 1.0, 1.0, json!({ "tags": [tag("g1", "Time Loop", 3.0)] })),
            read_vn("Other one", 0.4, 0.2, json!({ "tags": [tag("g2", "Comedy", 3.0)] })),
        ];
        let candidates = vec![
            json!({ "id": "v2", "title": "Weak fit", "rating": 90, "tags": [tag("g2", "Comedy", 2.0)] }),
            json!({ "id": "v1", "title": "Great fit", "rating": 80, "votecount": 500, "olang": "ja", "released": "2018-06-29", "length_minutes": 3000,
                "developers": [{ "id": "p1", "name": "Key" }], "image": { "url": "cover.jpg", "sexual": 0, "violence": 0 },
                "tags": [tag("g1", "Time Loop", 3.0), tag("g2", "Comedy", 2.0)] }),
            json!({ "id": "v3", "title": "No fit", "rating": 95, "tags": [tag("g9", "Mecha", 3.0)] }),
        ];

        let ranked = rank(&candidates, &tastes, &read);
        assert_eq!(ranked.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["v1", "v2"]);
        let best = &ranked[0];
        assert_eq!(best.because, "Loved one");
        assert_eq!(best.matched, ["Time Loop", "Comedy"]);
        assert_eq!(best.year, "2018");
        assert_eq!(best.rating, 8.0);
        assert_eq!(best.votes, 500);
        assert_eq!(best.language, "ja");
        assert_eq!(best.developer, "Key");
    }

    #[test]
    fn a_liked_writer_lifts_a_vn_and_is_named() {
        let writer = json!([{ "id": "s1", "name": "Great Writer", "role": "scenario" }]);
        let tastes = vec![taste("g1", "Time Loop", 3.0)];
        let read = vec![
            read_vn("First", 1.0, 1.0, json!({ "staff": writer, "tags": [tag("g1", "Time Loop", 3.0)] })),
            read_vn("Second", 1.0, 1.0, json!({ "staff": writer, "tags": [tag("g1", "Time Loop", 3.0)] })),
        ];
        let candidates = vec![
            json!({ "id": "v1", "title": "Same tags", "rating": 80, "tags": [tag("g1", "Time Loop", 2.0)] }),
            json!({ "id": "v2", "title": "Same tags and writer", "rating": 80, "staff": writer, "tags": [tag("g1", "Time Loop", 2.0)] }),
            json!({ "id": "v3", "title": "Same writer only", "rating": 80, "staff": writer, "tags": [] }),
        ];

        let ranked = rank(&candidates, &tastes, &read);
        assert_eq!(ranked[0].id, "v2");
        assert_eq!(ranked[0].credits, ["Great Writer"]);
        assert!(ranked[0].signals.staff > 0.0 && ranked[0].signals.tags > 0.0);
        assert!(ranked.iter().any(|r| r.id == "v3" && r.matched.is_empty()));
    }

    #[test]
    fn no_taste_means_no_recommendation() {
        assert!(rank(&[json!({ "id": "v1", "tags": [] })], &[], &[]).is_empty());
    }
}
