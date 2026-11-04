use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use reqwest::Method;
use serde_json::{json, Value};

const API: &str = "https://api.vndb.org/kana";
const USER_AGENT: &str = concat!("ShioriRPC/", env!("CARGO_PKG_VERSION"));
const MIN_INTERVAL: Duration = Duration::from_millis(500);
const FIELDS: &str =
    "id,title,alttitle,titles.title,titles.latin,aliases,released,rating,length_minutes,image.url,image.dims,image.sexual,image.violence";

pub const LIST_LABELS: [(&str, i64); 4] = [
    ("playing", 1),
    ("finished", 2),
    ("stalled", 3),
    ("dropped", 4),
];
const WISHLIST_LABEL: i64 = 5;
const FINISHED_LABEL: i64 = 2;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct VnResult {
    pub id: String,
    pub title: String,
    pub alt_title: String,
    pub year: String,
    pub image_url: String,
    pub nsfw: bool,
    pub rating: f64,
    pub length_minutes: i64,
    pub other_titles: Vec<String>,
}

pub(crate) fn is_nsfw(image: &Value) -> bool {
    let sexual = image["sexual"].as_f64().unwrap_or(0.0);
    let violence = image["violence"].as_f64().unwrap_or(0.0);
    sexual >= 1.0 || violence >= 1.0
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or("").to_string()
}

fn other_titles(o: &Value) -> Vec<String> {
    let titles = o["titles"].as_array().into_iter().flatten();
    let names = titles
        .flat_map(|t| [&t["title"], &t["latin"]])
        .chain(o["aliases"].as_array().into_iter().flatten())
        .chain([&o["alttitle"]])
        .map(text);

    let mut out: Vec<String> = Vec::new();
    for name in names {
        if !name.is_empty() && name != text(&o["title"]) && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

impl VnResult {
    fn from_api(o: &Value) -> VnResult {
        let released = text(&o["released"]);

        VnResult {
            id: text(&o["id"]),
            title: text(&o["title"]),
            alt_title: text(&o["alttitle"]),
            year: released.chars().take(4).collect(),
            image_url: text(&o["image"]["url"]),
            nsfw: is_nsfw(&o["image"]),
            rating: o["rating"].as_f64().unwrap_or(0.0),
            length_minutes: o["length_minutes"].as_i64().unwrap_or(0),
            other_titles: other_titles(o),
        }
    }

    pub fn url(&self) -> String {
        format!("https://vndb.org/{}", self.id)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReleaseCover {
    pub url: String,
    pub kind: String,
    pub release_title: String,
    pub nsfw: bool,
}

pub struct Vndb {
    http: reqwest::blocking::Client,
    last_call: Mutex<Instant>,
    cache_dir: PathBuf,
    covers_dir: PathBuf,
    token_users: Mutex<HashMap<String, Value>>,
}

fn hash16(text: &str) -> String {
    sha1_smol::Sha1::from(text).digest().to_string()[..16].to_string()
}

fn fresh(path: &Path, days: u64) -> bool {
    let age = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok());

    age.is_some_and(|age| age < Duration::from_secs(days * 86400))
}

pub(crate) fn read_cached(path: &Path, days: u64) -> Option<Value> {
    if !fresh(path, days) {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn has_length(raw: &Value) -> bool {
    raw.get("length_minutes").is_some() && raw.get("titles").is_some()
}

fn vn_id(id: &str) -> String {
    let id = id.trim();
    if id.starts_with('v') {
        id.to_string()
    } else {
        format!("v{id}")
    }
}

impl Vndb {
    pub fn new(data_dir: &Path) -> Vndb {
        let http = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(15))
            .build()
            .expect("http client");

        Vndb {
            http,
            last_call: Mutex::new(Instant::now() - MIN_INTERVAL),
            cache_dir: data_dir.join("cache").join("vndb"),
            covers_dir: data_dir.join("cache").join("covers"),
            token_users: Mutex::new(HashMap::new()),
        }
    }

    fn throttle(&self) {
        let mut last = self.last_call.lock().unwrap();
        let since = last.elapsed();
        if since < MIN_INTERVAL {
            std::thread::sleep(MIN_INTERVAL - since);
        }
        *last = Instant::now();
    }

    fn request(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<&Value>,
        token: &str,
    ) -> Result<Value, String> {
        for attempt in 0..4u64 {
            self.throttle();

            let mut req = self
                .http
                .request(method.clone(), format!("{API}{endpoint}"));
            if let Some(body) = body {
                req = req.json(body);
            }
            if !token.is_empty() {
                req = req.header("Authorization", format!("Token {token}"));
            }

            let resp = match req.send() {
                Ok(resp) => resp,
                Err(e) if attempt == 3 => return Err(format!("network error: {e}")),
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(1500 * (attempt + 1)));
                    continue;
                }
            };

            match resp.status().as_u16() {
                429 => std::thread::sleep(Duration::from_secs(2 * (attempt + 1))),
                401 => return Err("the VNDB token is invalid or was revoked".into()),
                code if code >= 400 => {
                    let detail: String =
                        resp.text().unwrap_or_default().chars().take(200).collect();
                    return Err(format!("HTTP {code}: {detail}"));
                }
                _ => {
                    let bytes = resp.bytes().map_err(|e| e.to_string())?;
                    if bytes.is_empty() {
                        return Ok(Value::Null);
                    }
                    return serde_json::from_slice(&bytes).map_err(|e| e.to_string());
                }
            }
        }
        Err("rate limited, giving up".into())
    }

    pub(crate) fn post(&self, endpoint: &str, body: &Value) -> Result<Value, String> {
        self.request(Method::POST, endpoint, Some(body), "")
    }

    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<VnResult>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(vec![]);
        }

        let cache = self
            .cache_dir
            .join(format!("{}.json", hash16(&query.to_lowercase())));
        if let Some(Value::Array(raw)) = read_cached(&cache, 7) {
            if raw.iter().all(has_length) {
                return Ok(raw.iter().map(VnResult::from_api).collect());
            }
        }

        let body = json!({
            "filters": ["search", "=", query],
            "fields": FIELDS,
            "results": limit.clamp(1, 25),
            "sort": "searchrank",
        });
        let data = self.post("/vn", &body)?;
        let results = data["results"].as_array().cloned().unwrap_or_default();

        let _ = fs::create_dir_all(&self.cache_dir);
        let _ = fs::write(&cache, serde_json::to_string(&results).unwrap_or_default());

        Ok(results.iter().map(VnResult::from_api).collect())
    }

    pub fn get(&self, id: &str) -> Result<Option<VnResult>, String> {
        let id = vn_id(id);
        let cache = self.cache_dir.join(format!("id_{id}.json"));
        if let Some(raw) = read_cached(&cache, 7).filter(has_length) {
            return Ok(Some(VnResult::from_api(&raw)));
        }

        let body = json!({"filters": ["id", "=", id], "fields": FIELDS, "results": 1});
        let data = self.post("/vn", &body)?;
        let Some(first) = data["results"].get(0).cloned() else {
            return Ok(None);
        };

        let _ = fs::create_dir_all(&self.cache_dir);
        let _ = fs::write(&cache, first.to_string());
        Ok(Some(VnResult::from_api(&first)))
    }

    pub fn description(&self, id: &str) -> Result<String, String> {
        let id = vn_id(id);
        let cache = self.cache_dir.join(format!("desc_{id}.json"));
        if let Some(raw) = read_cached(&cache, 30) {
            return Ok(text(&raw["description"]));
        }

        let body = json!({"filters": ["id", "=", id], "fields": "description", "results": 1});
        let data = self.post("/vn", &body)?;
        let first = data["results"].get(0).cloned().unwrap_or_else(|| json!({}));

        let _ = fs::create_dir_all(&self.cache_dir);
        let _ = fs::write(&cache, first.to_string());
        Ok(text(&first["description"]))
    }

    pub fn characters(&self, id: &str) -> Result<Value, String> {
        let id = vn_id(id);
        let cache = self.cache_dir.join(format!("chars_{id}.json"));
        if let Some(raw) = read_cached(&cache, 30) {
            return Ok(raw);
        }

        let body = json!({
            "filters": ["vn", "=", ["id", "=", id]],
            "fields": "id,name,original,description,image.url,image.sexual,blood_type,height,weight,bust,waist,hips,age,birthday,sex,vns.id,vns.role,vns.spoiler",
            "results": 100,
            "sort": "name",
        });
        let data = self.post("/character", &body)?;
        let results = data["results"].clone();

        let _ = fs::create_dir_all(&self.cache_dir);
        let _ = fs::write(&cache, results.to_string());
        Ok(results)
    }

    pub fn release_covers(&self, id: &str) -> Result<Vec<ReleaseCover>, String> {
        let id = vn_id(id);
        let cache = self.cache_dir.join(format!("releases_{id}.json"));

        let releases = match read_cached(&cache, 7) {
            Some(Value::Array(raw)) => raw,
            _ => {
                let body = json!({
                    "filters": ["vn", "=", ["id", "=", id]],
                    "fields": "id,title,released,images.url,images.type,images.sexual,images.violence",
                    "results": 100,
                    "sort": "released",
                });
                let data = self.post("/release", &body)?;
                let results = data["results"].as_array().cloned().unwrap_or_default();

                let _ = fs::create_dir_all(&self.cache_dir);
                let _ = fs::write(&cache, serde_json::to_string(&results).unwrap_or_default());
                results
            }
        };

        let mut covers = Vec::new();
        for release in &releases {
            for image in release["images"].as_array().into_iter().flatten() {
                let url = text(&image["url"]);
                if url.is_empty() {
                    continue;
                }
                covers.push(ReleaseCover {
                    url,
                    kind: text(&image["type"]),
                    release_title: text(&release["title"]),
                    nsfw: is_nsfw(image),
                });
            }
        }
        Ok(covers)
    }

    pub fn cover_path(&self, stem: &str, image_url: &str) -> Option<PathBuf> {
        if image_url.is_empty() {
            return None;
        }

        let file_name = image_url.rsplit('/').next().unwrap_or("");
        let ext = file_name
            .rsplit_once('.')
            .map(|(_, ext)| ext.split('?').next().unwrap_or("jpg"))
            .unwrap_or("jpg");
        let ext: String = ext.chars().take(4).collect();

        let dest = self.covers_dir.join(format!("{stem}.{ext}"));
        if fs::metadata(&dest).map(|m| m.len() > 0).unwrap_or(false) {
            return Some(dest);
        }

        let bytes = self.download(image_url).ok()?;
        let _ = fs::create_dir_all(&self.covers_dir);
        fs::write(&dest, &bytes).ok()?;
        Some(dest)
    }

    pub fn download(&self, url: &str) -> Result<Vec<u8>, String> {
        self.throttle();
        let resp = self
            .http
            .get(url)
            .timeout(Duration::from_secs(20))
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?;
        resp.bytes().map(|b| b.to_vec()).map_err(|e| e.to_string())
    }

    pub fn token_user(&self, token: &str) -> Result<Value, String> {
        if let Some(user) = self.token_users.lock().unwrap().get(token) {
            return Ok(user.clone());
        }
        let user = self.request(Method::GET, "/authinfo", None, token)?;
        self.token_users
            .lock()
            .unwrap()
            .insert(token.to_string(), user.clone());
        Ok(user)
    }

    pub fn list_user(&self, token: &str) -> Result<Value, String> {
        let user = self.token_user(token)?;
        let can_write = user["permissions"]
            .as_array()
            .is_some_and(|perms| perms.iter().any(|p| p == "listwrite"));

        if can_write {
            Ok(user)
        } else {
            Err("this token can't edit your list (it needs list write access)".into())
        }
    }

    pub fn list_entry(&self, token: &str, id: &str) -> Result<Option<Value>, String> {
        let user = self.list_user(token)?;
        let body = json!({
            "user": user["id"],
            "filters": ["id", "=", vn_id(id)],
            "fields": "labels.id,started,finished,vote",
        });
        let data = self.request(Method::POST, "/ulist", Some(&body), token)?;
        Ok(data["results"].get(0).cloned())
    }

    pub fn update_list_entry(&self, token: &str, id: &str, changes: &Value) -> Result<(), String> {
        let endpoint = format!("/ulist/{}", vn_id(id));
        self.request(Method::PATCH, &endpoint, Some(changes), token)?;
        Ok(())
    }

    pub fn list_votes(&self, token: &str) -> Result<Vec<(String, i64)>, String> {
        let user = self.token_user(token)?;
        let mut out = Vec::new();
        for page in 1..=50 {
            let body = json!({
                "user": user["id"],
                "fields": "vote",
                "sort": "id",
                "results": 100,
                "page": page,
            });
            let data = self.request(Method::POST, "/ulist", Some(&body), token)?;
            for entry in data["results"].as_array().into_iter().flatten() {
                out.push((text(&entry["id"]), entry["vote"].as_i64().unwrap_or(0)));
            }
            if data["more"].as_bool() != Some(true) {
                break;
            }
        }
        Ok(out)
    }

    pub fn wishlist(&self, token: &str) -> Result<Vec<VnResult>, String> {
        let list = self.labelled(token, WISHLIST_LABEL, "")?;
        Ok(list.into_iter().map(|(vn, _)| vn).collect())
    }

    pub fn finished_list(&self, token: &str) -> Result<Vec<(VnResult, Value)>, String> {
        self.labelled(token, FINISHED_LABEL, "finished,vote")
    }

    fn labelled(
        &self,
        token: &str,
        label: i64,
        extra: &str,
    ) -> Result<Vec<(VnResult, Value)>, String> {
        let user = self.token_user(token)?;
        let mut fields: Vec<String> = FIELDS.split(',').map(|f| format!("vn.{f}")).collect();
        if !extra.is_empty() {
            fields.push(extra.to_string());
        }
        let fields = fields.join(",");

        let mut out = Vec::new();
        for page in 1..=50 {
            let body = json!({
                "user": user["id"],
                "filters": ["label", "=", label],
                "fields": fields,
                "sort": "id",
                "results": 100,
                "page": page,
            });
            let data = self.request(Method::POST, "/ulist", Some(&body), token)?;

            for entry in data["results"].as_array().into_iter().flatten() {
                let mut result = VnResult::from_api(&entry["vn"]);
                result.id = text(&entry["id"]);
                out.push((result, entry.clone()));
            }

            if data["more"].as_bool() != Some(true) {
                break;
            }
        }
        Ok(out)
    }
}

pub fn push_list_status(
    vndb: &Vndb,
    token: &str,
    id: &str,
    status: &str,
    keep_existing: bool,
) -> Result<String, String> {
    let entry = vndb.list_entry(token, id)?.unwrap_or(Value::Null);
    let label_ids: Vec<i64> = entry["labels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|label| label["id"].as_i64())
        .collect();

    let current = LIST_LABELS
        .iter()
        .find(|(_, label)| label_ids.contains(label))
        .map(|(name, _)| name.to_string())
        .unwrap_or_default();

    if current == status || (!current.is_empty() && keep_existing) {
        return Ok(current);
    }

    let all_labels: Vec<i64> = LIST_LABELS.iter().map(|(_, label)| *label).collect();
    if status.is_empty() {
        vndb.update_list_entry(token, id, &json!({"labels_unset": all_labels}))?;
        return Ok(String::new());
    }

    let Some((_, label)) = LIST_LABELS.iter().find(|(name, _)| *name == status) else {
        return Err(format!("unknown status {status}"));
    };
    let mut others: Vec<i64> = all_labels.iter().copied().filter(|l| l != label).collect();
    others.push(WISHLIST_LABEL);

    let mut changes = json!({"labels_set": [label], "labels_unset": others});
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if entry["started"].is_null() {
        changes["started"] = json!(today);
    }
    if status == "finished" && entry["finished"].is_null() {
        changes["finished"] = json!(today);
    }

    vndb.update_list_entry(token, id, &changes)?;
    Ok(status.to_string())
}

pub fn similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    2.0 * matching_chars(&a, &b) as f64 / (a.len() + b.len()) as f64
}

fn matching_chars(a: &[char], b: &[char]) -> usize {
    let (mut best, mut a_start, mut b_start) = (0, 0, 0);
    let mut previous = vec![0usize; b.len() + 1];

    for i in 0..a.len() {
        let mut current = vec![0usize; b.len() + 1];
        for j in 0..b.len() {
            if a[i] == b[j] {
                current[j + 1] = previous[j] + 1;
                if current[j + 1] > best {
                    best = current[j + 1];
                    a_start = i + 1 - best;
                    b_start = j + 1 - best;
                }
            }
        }
        previous = current;
    }

    if best == 0 {
        return 0;
    }

    let left = matching_chars(&a[..a_start], &b[..b_start]);
    let right = matching_chars(&a[a_start + best..], &b[b_start + best..]);
    best + left + right
}

pub fn best_match(results: Vec<VnResult>, query: &str) -> Option<VnResult> {
    use crate::title_parser::loose;

    let q = loose(query);
    if q.is_empty() {
        return results.into_iter().next();
    }

    let mut best: Option<(f64, &VnResult)> = None;
    for vn in &results {
        let score = similarity(&q, &loose(&vn.title)).max(similarity(&q, &loose(&vn.alt_title)));
        if score > best.map_or(0.0, |(s, _)| s) {
            best = Some((score, vn));
        }
    }

    match best {
        Some((score, vn)) if score >= 0.6 => Some(vn.clone()),
        _ => results
            .iter()
            .max_by(|a, b| a.rating.total_cmp(&b.rating))
            .cloned(),
    }
}
