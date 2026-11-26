use crate::clock::Stamp;
use crate::net;
use serde_json::Value;
use std::path::Path;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPOSITORY: &str = "TiTi-xu912/cursor-done-notify";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateInfo {
    pub tag: String,
    pub url: String,
    pub notified_tag: String,
    pub checked: Option<Stamp>,
}

/// Up to four numeric parts, like System.Version: a missing part sorts below 0.
fn parse_version(text: &str) -> Option<[i64; 4]> {
    let clean = text.trim().trim_start_matches(['v', 'V']);
    let clean = clean.split('-').next().unwrap_or("");
    let parts: Vec<&str> = clean.split('.').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }
    let mut out = [-1i64; 4];
    for (i, part) in parts.iter().enumerate() {
        out[i] = part.parse::<u32>().ok()? as i64;
    }
    if parts.len() == 1 {
        out[1] = 0;
    }
    Some(out)
}

pub fn is_newer(tag: &str, current: &str) -> bool {
    matches!((parse_version(tag), parse_version(current)), (Some(a), Some(b)) if a > b)
}

impl UpdateInfo {
    pub fn newer(&self) -> bool {
        is_newer(&self.tag, VERSION)
    }

    pub fn load(path: &Path) -> Option<UpdateInfo> {
        let text = std::fs::read_to_string(path).ok()?;
        let parts: Vec<&str> = text.trim().split('|').collect();
        if parts.len() < 4 {
            return None;
        }
        Some(UpdateInfo {
            checked: Some(Stamp::from_dotnet_ticks(parts[0].parse().ok()?)?),
            tag: parts[1].to_string(),
            url: parts[2].to_string(),
            notified_tag: parts[3].to_string(),
        })
    }

    pub fn save(&self, path: &Path) {
        let ticks = self.checked.map_or(0, |c| c.to_dotnet_ticks());
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, format!("{ticks}|{}|{}|{}", self.tag, self.url, self.notified_tag));
    }

    pub fn parse(json: &str, previous: Option<&UpdateInfo>) -> UpdateInfo {
        let mut info = UpdateInfo {
            checked: Some(Stamp::now()),
            notified_tag: previous.map(|p| p.notified_tag.clone()).unwrap_or_default(),
            ..UpdateInfo::default()
        };
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(json) {
            if let Some(Value::String(tag)) = map.get("tag_name") {
                info.tag = tag.clone();
            }
            if let Some(Value::String(url)) = map.get("html_url") {
                info.url = url.clone();
            }
        }
        // Only ever open GitHub release pages, whatever the API says.
        if !info.url.to_lowercase().starts_with("https://github.com/") {
            info.url = format!("https://github.com/{REPOSITORY}/releases/latest");
        }
        info.tag = info.tag.replace('|', "");
        info.url = info.url.replace('|', "");
        info
    }

    pub fn fetch(previous: Option<&UpdateInfo>) -> UpdateInfo {
        let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
        let agent = format!("cursor-done-notify/{VERSION}");
        let result = net::send(&net::Request {
            method: "GET",
            url: &url,
            headers: &[("User-Agent", &agent), ("Accept", "application/vnd.github+json")],
            body: None,
            timeout_ms: 8_000,
        });
        match result {
            Ok(body) => UpdateInfo::parse(&body, previous),
            Err(_) => {
                let mut info = previous.cloned().unwrap_or_default();
                info.checked = Some(Stamp::now());
                info
            }
        }
    }
}
