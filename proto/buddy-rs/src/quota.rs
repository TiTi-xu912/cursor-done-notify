use crate::clock::Stamp;
use crate::lang::{self, english, t};
use crate::net;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct QuotaSnapshot {
    pub ok: bool,
    pub error: String,
    pub plan_name: String,
    pub auto_percent: Option<f64>,
    pub api_percent: Option<f64>,
    pub total_percent: Option<f64>,
    pub total_spend_cents: Option<i64>,
    pub included_spend_cents: Option<i64>,
    pub bonus_spend_cents: Option<i64>,
    pub limit_cents: Option<i64>,
    pub on_demand_used_cents: Option<i64>,
    pub on_demand_limit_cents: Option<i64>,
    pub cycle_start: Option<Stamp>,
    pub cycle_end: Option<Stamp>,
    pub fetched_at: Option<Stamp>,
}

impl QuotaSnapshot {
    pub fn failure(error: String, now: Stamp) -> QuotaSnapshot {
        QuotaSnapshot { ok: false, error, fetched_at: Some(now), ..QuotaSnapshot::default() }
    }

    pub fn headline_percent(&self) -> Option<f64> {
        if let Some(p) = self.total_percent {
            return Some(p);
        }
        match (self.total_spend_cents, self.limit_cents) {
            (Some(spent), Some(limit)) if limit > 0 => Some(spent as f64 * 100.0 / limit as f64),
            _ => None,
        }
    }

    pub fn days_left(&self, now: Stamp) -> Option<i64> {
        let end = self.cycle_end?;
        let days = end.secs_since(now) / 86_400.0;
        Some(if days < 0.0 { 0 } else { days.ceil() as i64 })
    }

    // Same key=value lines as the C# host, so both read each other's quota-cache.txt.
    pub fn serialize(&self) -> String {
        let num = |v: Option<f64>| v.map_or("-1".to_string(), |x| x.to_string());
        let int = |v: Option<i64>| v.unwrap_or(-1).to_string();
        let time = |v: Option<Stamp>| v.map_or("0".to_string(), |s| s.to_dotnet_ticks().to_string());
        let clean = |s: &str| s.replace(['\n', '\r'], " ");
        [
            ("ok", if self.ok { "1".to_string() } else { "0".to_string() }),
            ("error", clean(&self.error)),
            ("plan", clean(&self.plan_name)),
            ("auto", num(self.auto_percent)),
            ("api", num(self.api_percent)),
            ("total", num(self.total_percent)),
            ("spend", int(self.total_spend_cents)),
            ("included", int(self.included_spend_cents)),
            ("bonus", int(self.bonus_spend_cents)),
            ("limit", int(self.limit_cents)),
            ("ondemand", int(self.on_demand_used_cents)),
            ("ondemandLimit", int(self.on_demand_limit_cents)),
            ("start", time(self.cycle_start)),
            ("end", time(self.cycle_end)),
            ("fetched", time(self.fetched_at)),
        ]
        .iter()
        .map(|(k, v)| format!("{k}={v}\n"))
        .collect()
    }

    pub fn deserialize(text: &str) -> Option<QuotaSnapshot> {
        let mut map = std::collections::HashMap::new();
        for line in text.split('\n') {
            if let Some((k, v)) = line.split_once('=') {
                if !k.is_empty() {
                    map.insert(k.to_string(), v.trim_end_matches('\r').to_string());
                }
            }
        }
        let get = |k: &str| map.get(k).cloned().unwrap_or_default();
        let num = |k: &str| get(k).parse::<f64>().ok().filter(|v| *v >= 0.0);
        let int = |k: &str| get(k).parse::<i64>().ok().filter(|v| *v >= 0);
        let time = |k: &str| get(k).parse::<i64>().ok().and_then(Stamp::from_dotnet_ticks);
        let s = QuotaSnapshot {
            ok: get("ok") == "1",
            error: get("error"),
            plan_name: get("plan"),
            auto_percent: num("auto"),
            api_percent: num("api"),
            total_percent: num("total"),
            total_spend_cents: int("spend"),
            included_spend_cents: int("included"),
            bonus_spend_cents: int("bonus"),
            limit_cents: int("limit"),
            on_demand_used_cents: int("ondemand"),
            on_demand_limit_cents: int("ondemandLimit"),
            cycle_start: time("start"),
            cycle_end: time("end"),
            fetched_at: time("fetched"),
        };
        s.fetched_at.is_some().then_some(s)
    }
}

pub fn money(cents: i64) -> String {
    format!("${:.2}", cents as f64 / 100.0)
}

pub fn percent(value: Option<f64>) -> String {
    match value {
        None => "--".into(),
        Some(v) if v > 0.0 && v < 1.0 => "<1%".into(),
        Some(v) => format!("{}%", v.round() as i64),
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn epoch_ms(value: Option<&Value>) -> Option<Stamp> {
    number(value).filter(|ms| *ms > 0.0).map(|ms| Stamp(ms as i64))
}

pub fn membership_label(membership: &str) -> String {
    match membership.trim().to_lowercase().as_str() {
        "" => String::new(),
        "free" => "Free".into(),
        "free_trial" => t("Pro 试用", "Pro trial"),
        "pro" => "Pro".into(),
        "pro_plus" => "Pro+".into(),
        "ultra" => "Ultra".into(),
        "business" => "Teams".into(),
        "enterprise" => "Enterprise".into(),
        _ => membership.to_string(),
    }
}

pub fn parse(usage_json: &str, plan_json: &str, membership: &str, now: Stamp) -> QuotaSnapshot {
    let Ok(Value::Object(usage)) = serde_json::from_str::<Value>(usage_json) else {
        return QuotaSnapshot::failure(t("用量接口返回了无法识别的内容", "The usage API returned something unexpected"), now);
    };
    let mut s = QuotaSnapshot { ok: true, fetched_at: Some(now), ..QuotaSnapshot::default() };
    s.cycle_start = epoch_ms(usage.get("billingCycleStart"));
    s.cycle_end = epoch_ms(usage.get("billingCycleEnd"));
    if let Some(Value::Object(plan)) = usage.get("planUsage") {
        let pct = |k: &str| number(plan.get(k)).filter(|v| *v >= 0.0);
        let cents = |k: &str| number(plan.get(k)).filter(|v| *v >= 0.0).map(|v| v as i64);
        s.auto_percent = pct("autoPercentUsed");
        s.api_percent = pct("apiPercentUsed");
        s.total_percent = pct("totalPercentUsed");
        s.total_spend_cents = cents("totalSpend");
        s.included_spend_cents = cents("includedSpend");
        s.bonus_spend_cents = cents("bonusSpend");
        s.limit_cents = cents("limit");
    }
    if let Some(Value::Object(spend)) = usage.get("spendLimitUsage") {
        let (mut limit, mut used) = (number(spend.get("individualLimit")), number(spend.get("individualUsed")));
        if limit.is_none_or(|l| l < 0.0) {
            limit = number(spend.get("pooledLimit"));
            used = number(spend.get("pooledUsed"));
        }
        if let Some(limit) = limit.filter(|l| *l > 0.0) {
            s.on_demand_limit_cents = Some(limit as i64);
            s.on_demand_used_cents = Some(used.filter(|u| *u >= 0.0).unwrap_or(0.0) as i64);
        }
    }
    if let Ok(Value::Object(root)) = serde_json::from_str::<Value>(plan_json) {
        if let Some(Value::Object(info)) = root.get("planInfo") {
            if let Some(Value::String(name)) = info.get("planName") {
                s.plan_name = name.clone();
            }
        }
    }
    if s.plan_name.is_empty() {
        s.plan_name = membership_label(membership);
    }
    s
}

// ---- Cursor's own sign-in database ----

pub fn default_database() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Application Support")).unwrap_or_default();
    #[cfg(not(any(windows, target_os = "macos")))]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")).unwrap_or_default();
    base.join("Cursor").join("User").join("globalStorage").join("state.vscdb")
}

mod sqlite {
    use std::ffi::{c_char, c_int, c_void};

    // Windows ships winsqlite3.dll and macOS ships libsqlite3, so no SQLite is bundled.
    // "system" is stdcall on 32-bit Windows, which winsqlite3 uses, and plain C elsewhere.
    #[cfg_attr(windows, link(name = "winsqlite3", kind = "raw-dylib"))]
    #[cfg_attr(not(windows), link(name = "sqlite3"))]
    extern "system" {
        pub fn sqlite3_open_v2(filename: *const c_char, db: *mut *mut c_void, flags: c_int, vfs: *const c_char) -> c_int;
        pub fn sqlite3_busy_timeout(db: *mut c_void, ms: c_int) -> c_int;
        pub fn sqlite3_prepare_v2(db: *mut c_void, sql: *const c_char, bytes: c_int, stmt: *mut *mut c_void, tail: *mut *const c_char) -> c_int;
        pub fn sqlite3_bind_text(stmt: *mut c_void, index: c_int, value: *const c_char, bytes: c_int, destructor: isize) -> c_int;
        pub fn sqlite3_step(stmt: *mut c_void) -> c_int;
        pub fn sqlite3_column_blob(stmt: *mut c_void, column: c_int) -> *const c_void;
        pub fn sqlite3_column_bytes(stmt: *mut c_void, column: c_int) -> c_int;
        pub fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
        pub fn sqlite3_close(db: *mut c_void) -> c_int;
    }
}

/// Reads one value from state.vscdb's ItemTable; Ok(None) when the key is absent.
pub fn read_state(path: &Path, key: &str) -> Result<Option<String>, String> {
    use sqlite::*;
    use std::ffi::{c_char, CString};
    const OK: i32 = 0;
    const ROW: i32 = 100;
    const READ_ONLY: i32 = 1;
    const TRANSIENT: isize = -1;
    let cpath = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let sql = CString::new("SELECT value FROM ItemTable WHERE key = ?1").unwrap();
    unsafe {
        let mut db = std::ptr::null_mut();
        if sqlite3_open_v2(cpath.as_ptr(), &mut db, READ_ONLY, std::ptr::null()) != OK {
            if !db.is_null() {
                sqlite3_close(db);
            }
            return Err(t("无法打开 state.vscdb", "cannot open state.vscdb"));
        }
        sqlite3_busy_timeout(db, 2000);
        let mut stmt = std::ptr::null_mut();
        if sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) != OK {
            sqlite3_close(db);
            return Err(t("state.vscdb 结构不认识", "unrecognized state.vscdb layout"));
        }
        sqlite3_bind_text(stmt, 1, key.as_ptr() as *const c_char, key.len() as i32, TRANSIENT);
        let result = if sqlite3_step(stmt) == ROW {
            let len = sqlite3_column_bytes(stmt, 0);
            let ptr = sqlite3_column_blob(stmt, 0) as *const u8;
            if len <= 0 || ptr.is_null() {
                Some(String::new())
            } else {
                let bytes = std::slice::from_raw_parts(ptr, len as usize);
                Some(String::from_utf8_lossy(bytes).trim().trim_matches('"').to_string())
            }
        } else {
            None
        };
        sqlite3_finalize(stmt);
        sqlite3_close(db);
        Ok(result)
    }
}

// ---- background refresh ----

const USAGE_URL: &str = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";
const PLAN_URL: &str = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetPlanInfo";
const MIN_GAP: f64 = 20.0;
const URGENT_GAP: f64 = 3.0;
const INTERVAL: Duration = Duration::from_secs(5 * 60);
const PLAN_INTERVAL: f64 = 6.0 * 3600.0;

#[derive(Default)]
struct Wake {
    requested: bool,
    urgent: bool,
    stopping: bool,
}

struct Shared {
    latest: Mutex<Option<QuotaSnapshot>>,
    version: AtomicU32,
    wake: Mutex<Wake>,
    signal: Condvar,
    english: AtomicBool,
}

pub struct QuotaService {
    shared: Arc<Shared>,
}

impl QuotaService {
    pub fn start(database: PathBuf, cache: PathBuf) -> QuotaService {
        let shared = Arc::new(Shared {
            latest: Mutex::new(None),
            version: AtomicU32::new(0),
            wake: Mutex::new(Wake::default()),
            signal: Condvar::new(),
            english: AtomicBool::new(english()),
        });
        if let Some(cached) = std::fs::read_to_string(&cache).ok().and_then(|t| QuotaSnapshot::deserialize(&t)).filter(|s| s.ok) {
            *shared.latest.lock().unwrap() = Some(cached);
            shared.version.fetch_add(1, Ordering::SeqCst);
        }
        let worker = shared.clone();
        let _ = std::thread::Builder::new()
            .name("quota".into())
            .stack_size(256 * 1024)
            .spawn(move || run(worker, database, cache));
        QuotaService { shared }
    }

    pub fn latest(&self) -> Option<QuotaSnapshot> {
        self.shared.latest.lock().unwrap().clone()
    }

    pub fn version(&self) -> u32 {
        self.shared.version.load(Ordering::SeqCst)
    }

    pub fn request_refresh(&self, urgent: bool) {
        self.shared.english.store(english(), Ordering::Relaxed);
        let mut wake = self.shared.wake.lock().unwrap();
        wake.requested = true;
        wake.urgent |= urgent;
        self.shared.signal.notify_one();
    }

    pub fn sync_language(&self) {
        self.shared.english.store(english(), Ordering::Relaxed);
    }
}

impl Drop for QuotaService {
    fn drop(&mut self) {
        self.shared.wake.lock().unwrap().stopping = true;
        self.shared.signal.notify_one();
    }
}

fn run(shared: Arc<Shared>, database: PathBuf, cache: PathBuf) {
    let mut plan_json: Option<String> = None;
    let mut plan_fetched: Option<Stamp> = None;
    loop {
        let last = shared.latest.lock().unwrap().as_ref().and_then(|s| s.fetched_at);
        let since = last.map_or(f64::MAX, |l| Stamp::now().secs_since(l).max(0.0));
        let urgent = shared.wake.lock().unwrap().urgent;
        let gap = if urgent { URGENT_GAP } else { MIN_GAP };
        if since < gap {
            let wake = shared.wake.lock().unwrap();
            if wake.stopping {
                return;
            }
            let (mut wake, _) = shared.signal.wait_timeout(wake, Duration::from_secs_f64(gap - since)).unwrap();
            wake.requested = false;
            continue;
        }
        {
            let mut wake = shared.wake.lock().unwrap();
            if wake.stopping {
                return;
            }
            wake.urgent = false;
            wake.requested = false;
        }
        lang::set_english(shared.english.load(Ordering::Relaxed));
        let mut next = fetch_once(&database, &mut plan_json, &mut plan_fetched);
        let current = shared.latest.lock().unwrap().clone();
        match current {
            Some(cur) if !next.ok && cur.ok => {
                let stale = QuotaSnapshot { error: next.error.clone(), fetched_at: next.fetched_at, ..cur };
                next = stale;
            }
            _ if next.ok => next.error.clear(),
            _ => {}
        }
        if next.ok {
            let temp = cache.with_extension("txt.tmp");
            if std::fs::write(&temp, next.serialize()).is_ok() {
                let _ = std::fs::remove_file(&cache);
                let _ = std::fs::rename(&temp, &cache);
            }
        }
        *shared.latest.lock().unwrap() = Some(next);
        shared.version.fetch_add(1, Ordering::SeqCst);

        let wake = shared.wake.lock().unwrap();
        if wake.stopping {
            return;
        }
        if !wake.requested {
            let (wake, _) = shared.signal.wait_timeout(wake, INTERVAL).unwrap();
            if wake.stopping {
                return;
            }
        }
    }
}

pub fn fetch_once(database: &Path, plan_json: &mut Option<String>, plan_fetched: &mut Option<Stamp>) -> QuotaSnapshot {
    let now = Stamp::now();
    if !database.exists() {
        return QuotaSnapshot::failure(t("没找到 Cursor 的登录数据", "No Cursor sign-in data found"), now);
    }
    let read = |key: &str| read_state(database, key);
    let token = match read("cursorAuth/accessToken") {
        Ok(Some(token)) if !token.is_empty() => token,
        Ok(_) => return QuotaSnapshot::failure(t("Cursor 还没有登录", "Cursor is not signed in"), now),
        Err(e) => return QuotaSnapshot::failure(t("读不到 Cursor 登录数据：", "Cannot read Cursor sign-in data: ") + &e, now),
    };
    let membership = read("cursorAuth/stripeMembershipType").ok().flatten().unwrap_or_default();
    let auth = format!("Bearer {token}");
    let post = |url: &str| {
        net::send(&net::Request {
            method: "POST",
            url,
            headers: &[
                ("Authorization", &auth),
                ("Content-Type", "application/json"),
                ("Accept", "application/json"),
                ("Connect-Protocol-Version", "1"),
            ],
            body: Some("{}"),
            timeout_ms: 12_000,
        })
    };
    let usage = match post(USAGE_URL) {
        Ok(body) => body,
        Err(e) if matches!(e.status, Some(401) | Some(403)) => {
            return QuotaSnapshot::failure(t("登录已过期，打开 Cursor 会自动续上", "Sign-in expired; opening Cursor renews it"), now);
        }
        Err(e) => return QuotaSnapshot::failure(t("网络不通：", "Network error: ") + &e.message, now),
    };
    if plan_json.is_none() || plan_fetched.is_none_or(|at| now.secs_since(at) > PLAN_INTERVAL) {
        if let Ok(body) = post(PLAN_URL) {
            *plan_json = Some(body);
            *plan_fetched = Some(now);
        }
    }
    parse(&usage, plan_json.as_deref().unwrap_or(""), &membership, now)
}

// ---- rows under the bubble ----

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuotaRow {
    pub label: String,
    pub text: String,
    pub right: String,
    /// Percent for a meter row; None for plain text rows.
    pub bar: Option<f64>,
    pub warn: bool,
}

impl QuotaRow {
    fn note(text: String, warn: bool) -> QuotaRow {
        QuotaRow { text, warn, ..QuotaRow::default() }
    }

    fn meter(label: &str, pct: f64) -> QuotaRow {
        QuotaRow { label: label.into(), bar: Some(pct), text: percent(Some(pct)), ..QuotaRow::default() }
    }
}

pub fn rows(snapshot: Option<&QuotaSnapshot>, expanded: bool, now: Stamp) -> Vec<QuotaRow> {
    let mut head = QuotaRow { label: t("额度", "Usage"), ..QuotaRow::default() };
    let Some(s) = snapshot else {
        head.text = t("正在读取…", "Loading…");
        return vec![head];
    };
    if !s.ok {
        head.text = t("暂时取不到", "Unavailable");
        if !expanded {
            head.right = t("点开看原因", "click for why");
            return vec![head];
        }
        let tried = s.fetched_at.map(|f| f.local_hm()).unwrap_or_default();
        return vec![
            head,
            QuotaRow::note(s.error.clone(), true),
            QuotaRow::note(t("尝试于 ", "Tried at ") + &tried + &t(" · 点这里收起", " · click to collapse"), false),
        ];
    }

    let headline = s.headline_percent();
    head.bar = Some(headline.unwrap_or(-1.0));
    head.text = percent(headline);
    let mut right = Vec::new();
    if !expanded && s.api_percent.is_some() {
        right.push(format!("API {}", percent(s.api_percent)));
    }
    if let Some(days) = s.days_left(now) {
        right.push(if expanded {
            if english() { format!("resets in {days}d") } else { format!("{days} 天后重置") }
        } else if english() {
            format!("{days}d left")
        } else {
            format!("剩 {days} 天")
        });
    }
    head.right = right.join(" · ");
    let mut out = vec![head];
    if !expanded {
        return out;
    }

    let period = match (s.cycle_start, s.cycle_end) {
        (Some(a), Some(b)) => format!("{} – {}", a.local_md(), b.local_md()),
        _ => String::new(),
    };
    let plan = if s.plan_name.is_empty() { t("本期", "This cycle") } else { s.plan_name.clone() + &t(" 套餐", " plan") };
    out.push(QuotaRow::note(if period.is_empty() { plan } else { format!("{plan} · {period}") }, false));
    if let Some(p) = s.auto_percent {
        out.push(QuotaRow::meter("Auto", p));
    }
    if let Some(p) = s.api_percent {
        out.push(QuotaRow::meter("API", p));
    }
    let mut spend = Vec::new();
    match (s.included_spend_cents, s.limit_cents, s.total_spend_cents) {
        (Some(inc), Some(limit), _) if limit > 0 => spend.push(t("套餐内 ", "Included ") + &format!("{} / {}", money(inc), money(limit))),
        (_, _, Some(total)) => spend.push(t("已用 ", "Used ") + &money(total)),
        _ => {}
    }
    if let Some(bonus) = s.bonus_spend_cents.filter(|b| *b > 0) {
        spend.push(t("赠送 ", "Bonus ") + &money(bonus));
    }
    if !spend.is_empty() {
        out.push(QuotaRow::note(spend.join(" · "), false));
    }
    if let Some(limit) = s.on_demand_limit_cents.filter(|l| *l > 0) {
        let used = s.on_demand_used_cents.unwrap_or(0);
        let mut row = QuotaRow::meter(&t("按需", "Extra"), used as f64 * 100.0 / limit as f64);
        row.text = format!("{} / {}", money(used), money(limit));
        out.push(row);
    }
    let mut footer = t("更新于 ", "Updated ") + &s.fetched_at.map(|f| f.local_hm()).unwrap_or_default();
    if !s.error.is_empty() {
        footer += &t(" · 刷新失败", " · refresh failed");
    }
    out.push(QuotaRow::note(footer + &t(" · 点这里收起", " · click to collapse"), !s.error.is_empty()));
    out
}
