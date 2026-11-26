use crate::clock::{elapsed, Stamp};
use crate::event::HookEvent;
use crate::lang::{english, t};
pub use crate::quota::QuotaSnapshot;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---- text helpers ----

pub fn shorten(text: &str, max: usize) -> String {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= max {
        return clean;
    }
    clean.chars().take(max).collect::<String>() + "\u{2026}"
}

pub fn leaf(path: &str) -> String {
    if path.trim().is_empty() {
        return String::new();
    }
    let trimmed = path.trim_end_matches(['\\', '/']);
    let leaf = match trimmed.rfind(['\\', '/']) {
        Some(cut) => &trimmed[cut + 1..],
        None => trimmed,
    };
    if leaf.is_empty() { path.to_string() } else { leaf.to_string() }
}

pub use crate::quota::money;

// ---- settings ----

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub mute: bool,
    pub quota: bool,
    pub quiet: bool,
    pub theme: String,
    pub language: String,
    pub updates: bool,
    pub seconds: i64,
    pub stale_minutes: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            mute: false,
            quota: true,
            quiet: true,
            theme: "auto".into(),
            language: "auto".into(),
            updates: true,
            seconds: 0,
            stale_minutes: 0,
        }
    }
}

impl Settings {
    pub fn default_path() -> PathBuf {
        #[cfg(windows)]
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        #[cfg(target_os = "macos")]
        let base = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library").join("Application Support"))
            .unwrap_or_else(std::env::temp_dir);
        #[cfg(not(any(windows, target_os = "macos")))]
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(std::env::temp_dir);
        base.join("cursor-done-notify").join("settings.txt")
    }

    pub fn load(path: &Path) -> Settings {
        let mut s = Settings::default();
        let Ok(text) = std::fs::read_to_string(path) else { return s };
        for line in text.split('\n') {
            let Some(eq) = line.find('=') else { continue };
            if eq == 0 {
                continue;
            }
            let key = line[..eq].trim();
            let value = line[eq + 1..].trim();
            match key {
                "mute" => s.mute = value == "1",
                "quota" => s.quota = value != "0",
                "quiet" => s.quiet = value != "0",
                "updates" => s.updates = value != "0",
                "theme" if value == "light" || value == "dark" => s.theme = value.into(),
                "language" if value == "zh" || value == "en" => s.language = value.into(),
                "seconds" => s.seconds = value.parse().unwrap_or(0),
                "stale" => s.stale_minutes = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        s
    }

    pub fn save(&self, path: &Path) {
        let b = |v: bool| if v { "1" } else { "0" };
        let text = format!(
            "mute={}\nquota={}\nquiet={}\nupdates={}\ntheme={}\nlanguage={}\nseconds={}\nstale={}\n",
            b(self.mute),
            b(self.quota),
            b(self.quiet),
            b(self.updates),
            self.theme,
            self.language,
            self.seconds,
            self.stale_minutes
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

// ---- sessions ----

#[derive(Clone, Debug)]
pub struct Session {
    pub key: String,
    pub phase: String,
    pub headline: String,
    pub activity: String,
    pub pose: String,
    pub detail: String,
    pub prompt: String,
    pub project: String,
    pub snippet: String,
    pub accent: String,
    pub summary: String,
    pub generation_id: String,
    pub workspace_root: String,
    pub host_process: String,
    pub host_window: i64,
    pub started_at: Option<Stamp>,
    pub step_started_at: Option<Stamp>,
    pub done_until: Option<Stamp>,
    pub settle_until: Option<Stamp>,
    pub done_at: Option<Stamp>,
    pub updated_at: Stamp,
    pub steps: i64,
    pub open_tools: i64,
    pub open_subagents: i64,
    pub sound_played: bool,
    pub sound_pending: bool,
    pub stop_seen: bool,
    pub answer_seen: bool,
    pub spend_shared: bool,
    pub spend_start: Option<i64>,
    pub spend_end: Option<i64>,
    pub history: Vec<String>,
    pub open_tool_ids: HashSet<String>,
    pub completed_tool_ids: HashSet<String>,
    pub retired_generations: Vec<String>,
}

impl Session {
    fn new(key: &str, now: Stamp) -> Session {
        Session {
            key: key.to_string(),
            phase: "idle".into(),
            headline: t("点点在这儿", "Diandian is here"),
            activity: t("等你开始提问", "Waiting for your question"),
            pose: "idle".into(),
            detail: String::new(),
            prompt: String::new(),
            project: String::new(),
            snippet: String::new(),
            accent: "blue".into(),
            summary: String::new(),
            generation_id: String::new(),
            workspace_root: String::new(),
            host_process: String::new(),
            host_window: 0,
            started_at: None,
            step_started_at: None,
            done_until: None,
            settle_until: None,
            done_at: None,
            updated_at: now,
            steps: 0,
            open_tools: 0,
            open_subagents: 0,
            sound_played: false,
            sound_pending: false,
            stop_seen: false,
            answer_seen: false,
            spend_shared: false,
            spend_start: None,
            spend_end: None,
            history: Vec::new(),
            open_tool_ids: HashSet::new(),
            completed_tool_ids: HashSet::new(),
            retired_generations: Vec::new(),
        }
    }

    pub fn work_outstanding(&self) -> bool {
        self.open_tools > 0 || self.open_subagents > 0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub phase: String,
    pub accent: String,
    pub headline: String,
    pub activity: String,
    pub detail: String,
    pub meta: String,
    pub hint: String,
    pub pose: String,
    pub history: String,
    pub hint_alert: bool,
}

/// What is in front: process name, window title, window handle.
#[derive(Clone, Debug, Default)]
pub struct Foreground {
    pub process: String,
    pub title: String,
    pub window: String,
}

pub struct BuddyState {
    pub done_seconds: i64,
    pub stale_minutes: i64,
    pub doze_seconds: i64,
    pub settings: Settings,
    pub quota_alert_path: Option<PathBuf>,
    pub quota_notice: String,
    pub quota_notice_until: Option<Stamp>,
    pub pinned_key: String,
    pub foreground: Box<dyn Fn() -> Option<Foreground>>,
    pub sound: Box<dyn FnMut(&str)>,
    pub sessions: HashMap<String, Session>,
    pub order: Vec<String>,
    pub current: Option<String>,
    idle: Session,
}

const DETAIL_KEYS: [&str; 8] = ["command", "file_path", "path", "target_file", "pattern", "query", "url", "glob"];

impl BuddyState {
    pub fn new() -> BuddyState {
        let mut st = BuddyState {
            done_seconds: 12,
            stale_minutes: 180,
            doze_seconds: 90,
            settings: Settings::default(),
            quota_alert_path: None,
            quota_notice: String::new(),
            quota_notice_until: None,
            pinned_key: String::new(),
            foreground: Box::new(|| None),
            sound: Box::new(|_| {}),
            sessions: HashMap::new(),
            order: Vec::new(),
            current: None,
            idle: Session::new("", Stamp::now()),
        };
        st.apply_timing(&Settings::default());
        st
    }

    /// A choice made in the menu wins over the older environment variables.
    pub fn apply_timing(&mut self, settings: &Settings) {
        self.done_seconds = if settings.seconds > 0 {
            settings.seconds.clamp(3, 60)
        } else {
            env_int("CURSOR_NOTIFY_SECONDS", 12, 3, 60)
        };
        self.stale_minutes = if settings.stale_minutes > 0 {
            settings.stale_minutes.clamp(10, 1440)
        } else {
            env_int("CURSOR_NOTIFY_STALE_MINUTES", 180, 10, 1440)
        };
    }

    pub fn live(&self) -> &Session {
        self.current.as_ref().and_then(|k| self.sessions.get(k)).unwrap_or(&self.idle)
    }

    #[cfg(test)]
    pub fn live_mut(&mut self) -> &mut Session {
        match self.current.clone() {
            Some(k) if self.sessions.contains_key(&k) => self.sessions.get_mut(&k).unwrap(),
            _ => &mut self.idle,
        }
    }

    fn use_session(&mut self, key: &str, now: Stamp) -> String {
        let key = if key.trim().is_empty() { "default" } else { key };
        if !self.sessions.contains_key(key) {
            self.sessions.insert(key.to_string(), Session::new(key, now));
            self.order.push(key.to_string());
        }
        self.current = Some(key.to_string());
        key.to_string()
    }

    pub fn remove_session(&mut self, key: &str) {
        self.order.retain(|k| k != key);
        self.sessions.remove(key);
        if self.pinned_key == key {
            self.pinned_key.clear();
        }
        if self.current.as_deref() == Some(key) {
            self.current = self.order.last().cloned();
        }
    }

    fn trim_sessions(&mut self) {
        while self.order.len() > 8 {
            let victim = self
                .order
                .iter()
                .find(|k| matches!(self.sessions[*k].phase.as_str(), "done" | "idle"))
                .cloned();
            let Some(victim) = victim else { break };
            self.order.retain(|k| *k != victim);
            self.sessions.remove(&victim);
            if self.current.as_deref() == Some(victim.as_str()) {
                self.current = None;
            }
        }
        while self.order.len() > 16 {
            let oldest = self.order.iter().min_by_key(|k| self.sessions[*k].updated_at).cloned();
            let Some(oldest) = oldest else { break };
            self.remove_session(&oldest);
        }
    }

    fn other_session_running(&self, key: &str) -> bool {
        self.sessions
            .values()
            .any(|o| o.key != key && (o.phase == "running" || o.phase == "settling"))
    }

    // ---- event handling ----

    pub fn apply(&mut self, e: &HookEvent, now: Stamp) {
        let event_name = e.get("hook_event_name");
        if event_name.trim().is_empty() {
            return;
        }
        let mut id = e.get("conversation_id");
        if id.trim().is_empty() {
            id = e.get("generation_id");
        }
        let key = self.use_session(&id, now);
        let others = self.other_session_running(&key);
        let done_seconds = self.done_seconds;
        let s = self.sessions.get_mut(&key).unwrap();
        apply_to(s, e, &event_name, now, others, done_seconds);
        s.updated_at = now;
        self.trim_sessions();
    }

    // ---- lifecycle ----

    pub fn update_lifecycle(&mut self, now: Stamp) {
        let keys = self.order.clone();
        for key in keys {
            let others = self.other_session_running(&key);
            let done_seconds = self.done_seconds;
            let stale = self.stale_minutes;
            let Some(s) = self.sessions.get_mut(&key) else { continue };
            if s.phase == "settling" && s.settle_until.is_some_and(|u| now >= u) {
                soft_complete(s, now, others, done_seconds);
                s.updated_at = now;
            }
            if (s.phase == "running" || s.phase == "settling") && now >= s.updated_at.add_mins(stale as f64) {
                expire_stale_turn(s, now, done_seconds);
            }
        }
    }

    pub fn should_close(&self, now: Stamp) -> bool {
        if self.order.is_empty() {
            return false;
        }
        let mut latest: Option<Stamp> = None;
        for key in &self.order {
            let s = &self.sessions[key];
            if s.phase != "done" {
                return false;
            }
            if let Some(u) = s.done_until {
                latest = Some(latest.map_or(u, |l| l.max(u)));
            }
        }
        latest.is_some_and(|l| now >= l)
    }

    pub fn play_pending_sounds(&mut self, now: Stamp) {
        let mut chosen: Option<&Session> = None;
        for key in &self.order {
            let s = &self.sessions[key];
            if !s.sound_pending {
                continue;
            }
            let better = match chosen {
                None => true,
                Some(c) => s.accent == "red" || (s.accent == "orange" && c.accent != "red"),
            };
            if better {
                chosen = Some(s);
            }
        }
        let Some(chosen) = chosen else { return };
        let accent = chosen.accent.clone();
        let quiet = self.settings.quiet && self.session_focused(chosen);
        if quiet {
            let soon = now.add_secs(4.0);
            for s in self.sessions.values_mut() {
                if !s.sound_pending || s.done_until.is_some_and(|u| u <= soon) {
                    continue;
                }
                s.done_until = Some(soon);
            }
        }
        if !self.settings.mute && !quiet {
            let kind = match accent.as_str() {
                "red" => "error",
                "orange" => "warn",
                _ => "done",
            };
            (self.sound)(kind);
        }
        for s in self.sessions.values_mut() {
            if s.sound_pending {
                s.sound_pending = false;
                s.sound_played = true;
            }
        }
    }

    // ---- focus and jumping back ----

    pub fn terminal_host(s: &Session) -> bool {
        s.host_window != 0 && s.host_process != "Cursor"
    }

    pub fn can_jump(s: Option<&Session>) -> bool {
        s.is_some_and(|s| Self::terminal_host(s) || !leaf(&s.workspace_root).is_empty())
    }

    pub fn session_focused(&self, s: &Session) -> bool {
        let Some(fg) = (self.foreground)() else { return false };
        if Self::terminal_host(s) {
            return fg.window == s.host_window.to_string();
        }
        let name = leaf(&s.workspace_root);
        if name.is_empty() {
            return false;
        }
        // macOS only reveals window titles with Screen Recording permission; without one,
        // Cursor being in front is the best signal there is.
        if fg.process == "Cursor" && fg.title.is_empty() {
            return true;
        }
        fg.process == "Cursor" && fg.title.to_lowercase().contains(&name.to_lowercase())
    }

    /// Returns the session whose window should come forward, if any.
    pub fn click_session(&mut self, key: &str) -> Option<Session> {
        if key.trim().is_empty() {
            return None;
        }
        let s = self.sessions.get(key)?.clone();
        if s.phase == "done" {
            self.remove_session(key);
            return None;
        }
        if self.pinned_key == key {
            self.pinned_key.clear();
            return None;
        }
        self.pinned_key = key.to_string();
        Some(s)
    }

    // ---- presentation ----

    pub fn current_pose(&self, s: &Session, now: Stamp) -> String {
        if s.phase == "idle" {
            return "idle".into();
        }
        if s.phase == "done" || s.phase == "settling" {
            return match s.accent.as_str() {
                "red" => "error".into(),
                "orange" if s.pose == "sleep" => "sleep".into(),
                "orange" => "stop".into(),
                _ => "done".into(),
            };
        }
        if s.accent == "red" {
            return "error".into();
        }
        if s.phase == "running"
            && !s.work_outstanding()
            && s.step_started_at.is_some_and(|at| now.secs_since(at) >= self.doze_seconds as f64)
        {
            return "sleep".into();
        }
        if s.pose.trim().is_empty() { "think".into() } else { s.pose.clone() }
    }

    pub fn spend_text(&self, s: &Session) -> String {
        let (Some(start), Some(end)) = (s.spend_start, s.spend_end) else { return String::new() };
        let delta = end - start;
        if delta <= 0 {
            return String::new();
        }
        let prefix = if s.spend_shared { t("期间约 ", "about ") } else { t("本轮约 ", "this turn ~") };
        prefix + &money(delta)
    }

    pub fn meta_text(&self, s: &Session, now: Stamp) -> String {
        if s.phase == "done" {
            let spend = self.spend_text(s);
            return if spend.is_empty() { s.summary.clone() } else { format!("{} · {}", s.summary, spend) };
        }
        if s.started_at.is_some() {
            let steps = if english() { format!(" · step {}", s.steps) } else { format!(" · 第 {} 步", s.steps) };
            let text = t("已用 ", "") + &elapsed(s.started_at, now) + &steps;
            return if s.project.is_empty() { text } else { format!("{text} · {}", s.project) };
        }
        t("等待这一轮开始", "Waiting for a turn")
    }

    pub fn hint(&self, s: &Session, now: Stamp) -> String {
        let jump = Self::can_jump(Some(s));
        if s.phase == "done" {
            if let Some(until) = s.done_until {
                let left = (until.secs_since(now)).ceil().max(1.0) as i64;
                return if jump {
                    t("点标题跳回对话 · 点别处关掉 · ", "Title: back to chat · elsewhere: close · ") + &format!("{left}s")
                } else {
                    t("点一下关掉 · ", "Click to close · ") + &format!("{left}s")
                };
            }
        }
        match s.phase.as_str() {
            "settling" => t("回复已经写好了", "The reply is ready"),
            "running" if jump => t("点标题跳回对话 · 点别处看步骤", "Title: back to chat · elsewhere: steps"),
            "running" => t("点一下可以看步骤", "Click to see the steps"),
            _ => t("等你发来问题", "Waiting for your question"),
        }
    }

    fn session_label(s: &Session, key: &str) -> String {
        if !s.prompt.is_empty() {
            return shorten(&s.prompt, 16);
        }
        if !s.project.is_empty() {
            return shorten(&s.project, 16);
        }
        key.chars().take(6).collect()
    }

    fn session_line(&self, s: &Session, key: &str) -> String {
        let (mark, state) = if s.phase == "done" && s.accent == "red" {
            ("bad", t("出错", "failed"))
        } else if s.phase == "done" && s.accent == "orange" {
            ("warn", t("已中断", "stopped"))
        } else if s.phase == "done" {
            ("done", t("已完成", "done"))
        } else if s.phase == "settling" {
            ("wait", t("收尾", "wrapping up"))
        } else if s.accent == "orange" || s.accent == "red" {
            ("warn", t("这一步失败", "step failed"))
        } else if !s.activity.is_empty() {
            ("run", s.activity.clone())
        } else {
            ("run", t("进行中", "running"))
        };
        let extra = if s.phase == "running" && !s.detail.is_empty() { format!(" · {}", shorten(&s.detail, 18)) } else { String::new() };
        let prefix = if self.pinned_key == key { t("当前 · ", "pinned · ") } else { String::new() };
        format!("{mark}|{key}|{prefix}{} · {state}{extra}", Self::session_label(s, key))
    }

    fn rank(s: &Session) -> i32 {
        if s.phase == "done" && s.accent == "red" {
            0
        } else if s.phase == "running" || s.phase == "settling" {
            1
        } else if s.phase == "done" && s.accent == "orange" {
            2
        } else {
            3
        }
    }

    pub fn board_text(&self) -> String {
        let mut rows: Vec<&Session> = self.order.iter().map(|k| &self.sessions[k]).collect();
        rows.sort_by(|a, b| Self::rank(a).cmp(&Self::rank(b)).then(b.updated_at.cmp(&a.updated_at)));
        rows.iter().take(8).map(|s| self.session_line(s, &s.key)).collect::<Vec<_>>().join("\n")
    }

    pub fn presentation(&mut self, now: Stamp) -> View {
        if self.order.len() <= 1 {
            if self.order.len() == 1 {
                self.current = Some(self.order[0].clone());
            }
            let s = if self.order.len() == 1 { &self.sessions[&self.order[0]] } else { &self.idle };
            return View {
                phase: s.phase.clone(),
                accent: s.accent.clone(),
                headline: s.headline.clone(),
                activity: s.activity.clone(),
                detail: s.detail.clone(),
                meta: self.meta_text(s, now),
                hint: self.hint(s, now),
                pose: self.current_pose(s, now),
                history: s.history.join("\n"),
                hint_alert: false,
            };
        }

        let (mut running, mut bad, mut aborted, mut finished) = (0, 0, 0, 0);
        let mut newest_run: Option<&Session> = None;
        for key in &self.order {
            let s = &self.sessions[key];
            if s.phase == "done" && s.accent == "red" {
                bad += 1;
            } else if s.phase == "done" && s.accent == "orange" {
                aborted += 1;
            } else if s.phase == "done" {
                finished += 1;
            } else {
                running += 1;
                if newest_run.is_none_or(|n| s.updated_at >= n.updated_at) {
                    newest_run = Some(s);
                }
            }
        }
        let mut parts = Vec::new();
        let count = |n: i32, zh: &str, en: &str| if english() { format!("{n} {en}") } else { format!("{n} 个{zh}") };
        if running > 0 {
            parts.push(count(running, "进行中", "running"));
        }
        if bad > 0 {
            parts.push(count(bad, "出错", "failed"));
        }
        if aborted > 0 {
            parts.push(count(aborted, "已中断", "stopped"));
        }
        if finished > 0 {
            parts.push(count(finished, "已完成", "done"));
        }
        let mut view = View {
            headline: parts.join(" · "),
            accent: "green".into(),
            pose: "done".into(),
            phase: if running > 0 { "running".into() } else { "done".into() },
            ..View::default()
        };
        if bad > 0 {
            view.accent = "red".into();
            view.pose = "error".into();
        } else if running > 0 {
            view.accent = "blue".into();
            view.pose = newest_run.filter(|n| !n.pose.is_empty()).map_or("think".into(), |n| n.pose.clone());
        } else if aborted > 0 {
            view.accent = "orange".into();
            view.pose = "stop".into();
        }
        view.hint = t("点一行只处理这一路", "Click a row to follow it");

        let mut focus: Option<Session> = None;
        if !self.pinned_key.is_empty() {
            match self.sessions.get(&self.pinned_key) {
                Some(f) if f.phase == "done" => self.pinned_key.clear(),
                Some(f) => focus = Some(f.clone()),
                None => {}
            }
        }
        if let Some(f) = &focus {
            view.headline = Self::session_label(f, &f.key);
            view.activity = f.activity.clone();
            view.detail = f.detail.clone();
            view.accent = if f.accent.is_empty() { "blue".into() } else { f.accent.clone() };
            view.pose = if f.pose.is_empty() { "think".into() } else { f.pose.clone() };
            view.phase = "running".into();
            view.hint = t("再点这一行回到总览", "Click the row again for the overview");
        }
        if focus.is_none() && view.phase == "done" {
            let latest = self.sessions.values().filter_map(|s| s.done_until).max();
            let left = latest.map_or(1, |l| l.secs_since(now).ceil().max(1.0) as i64);
            view.hint = t("点一下关掉 · ", "Click to close · ") + &format!("{left}s");
        }
        view.meta = if english() { format!("{} sessions", self.order.len()) } else { format!("共 {} 个会话", self.order.len()) };
        view.history = self.board_text();
        view
    }

    // ---- quota: turn spend and alerts ----

    pub fn update_turn_spend(&mut self, snap: &QuotaSnapshot) {
        let (true, Some(total), Some(fetched)) = (snap.ok, snap.total_spend_cents, snap.fetched_at) else { return };
        if total < 0 {
            return;
        }
        for s in self.sessions.values_mut() {
            let Some(started) = s.started_at else { continue };
            if s.spend_start.is_none() && s.phase != "done" && fetched >= started.add_secs(-30.0) {
                s.spend_start = Some(total);
            }
            if s.phase == "done" && s.spend_start.is_some() && s.done_at.is_some_and(|d| fetched >= d) {
                s.spend_end = Some(total);
            }
        }
    }

    pub fn needs_spend_sample(&self) -> bool {
        self.sessions.values().any(|s| s.phase != "done" && s.started_at.is_some() && s.spend_start.is_none())
    }

    /// Cursor books the last requests a few seconds late, so keep sampling briefly after a turn ends.
    pub fn needs_spend_follow_up(&self, snap: Option<&QuotaSnapshot>, now: Stamp) -> bool {
        for s in self.sessions.values() {
            let Some(done_at) = s.done_at else { continue };
            if s.phase != "done" || s.spend_start.is_none() || now >= done_at.add_secs(15.0) {
                continue;
            }
            let fetched = snap.and_then(|q| q.fetched_at);
            if fetched.is_none_or(|f| f < done_at.add_secs(5.0)) {
                return true;
            }
        }
        false
    }

    pub fn update_quota_alert(&mut self, snap: &QuotaSnapshot, now: Stamp) {
        if !snap.ok {
            return;
        }
        let total = snap.headline_percent().unwrap_or(-1.0);
        let api = snap.api_percent.unwrap_or(-1.0);
        let api_wins = api > total;
        let percent = if api_wins { api } else { total };
        let level = if percent >= 100.0 { 100 } else if percent >= 80.0 { 80 } else { 0 };
        if level == 0 {
            return;
        }
        let cycle = snap.cycle_end.map_or("unknown".to_string(), utc_cycle_key);
        let mut alerted = 0;
        if let Some(path) = &self.quota_alert_path {
            if let Ok(text) = std::fs::read_to_string(path) {
                let saved: Vec<&str> = text.trim().split('|').collect();
                if saved.len() == 2 && saved[0] == cycle {
                    alerted = saved[1].parse().unwrap_or(0);
                }
            }
        }
        if level <= alerted {
            return;
        }
        if let Some(path) = &self.quota_alert_path {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, format!("{cycle}|{level}"));
        }
        let name = if api_wins { t("本期 API 额度", "API usage") } else { t("本期额度", "Usage") };
        self.quota_notice = if level >= 100 {
            name + &t("已用完", " is used up")
        } else {
            name + &t("已用 ", " at ") + &format!("{}%", percent.floor() as i64)
        };
        self.quota_notice_until = Some(now.add_secs(15.0));
        if !self.settings.mute {
            (self.sound)("alert");
        }
    }

    pub fn active_quota_notice(&self, now: Stamp) -> String {
        match self.quota_notice_until {
            Some(until) if !self.quota_notice.is_empty() && now < until => self.quota_notice.clone(),
            _ => String::new(),
        }
    }
}

fn env_int(name: &str, fallback: i64, min: i64, max: i64) -> i64 {
    match std::env::var(name) {
        Ok(text) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => {
            text.parse::<i64>().map_or(fallback, |v| v.clamp(min, max))
        }
        _ => fallback,
    }
}

/// UTC "yyyyMMddHH" for a billing-cycle end, used to remember which cycle was alerted.
fn utc_cycle_key(at: Stamp) -> String {
    let secs = at.0.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let hour = secs.rem_euclid(86_400) / 3600;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!("{year:04}{month:02}{day:02}{hour:02}")
}

// ---- per-session transitions ----

fn add_retired(s: &mut Session, generation: &str) {
    if generation.trim().is_empty() {
        return;
    }
    if !s.retired_generations.iter().any(|g| g == generation) {
        s.retired_generations.push(generation.to_string());
    }
    while s.retired_generations.len() > 16 {
        s.retired_generations.remove(0);
    }
}

fn add_history(s: &mut Session, line: &str) {
    let line = shorten(line, 72);
    if line.trim().is_empty() || s.history.last() == Some(&line) {
        return;
    }
    s.history.push(line);
    while s.history.len() > 5 {
        s.history.remove(0);
    }
}

fn reset_turn(s: &mut Session, now: Stamp, others_running: bool) {
    s.phase = "running".into();
    s.started_at = Some(now);
    s.step_started_at = Some(now);
    s.steps = 0;
    s.history.clear();
    s.snippet.clear();
    s.done_until = None;
    s.settle_until = None;
    s.sound_played = false;
    s.sound_pending = false;
    s.stop_seen = false;
    s.answer_seen = false;
    s.open_tools = 0;
    s.open_subagents = 0;
    s.open_tool_ids.clear();
    s.completed_tool_ids.clear();
    s.accent = "blue".into();
    s.summary.clear();
    s.headline = t("正在处理", "Working");
    s.activity.clear();
    s.detail.clear();
    s.pose = "think".into();
    s.spend_start = None;
    s.spend_end = None;
    s.spend_shared = others_running;
    s.done_at = None;
}

fn enter_running(s: &mut Session) {
    if s.stop_seen {
        return;
    }
    s.sound_played = false;
    s.sound_pending = false;
    s.phase = "running".into();
    s.settle_until = None;
    s.done_until = None;
    s.done_at = None;
    s.spend_end = None;
    s.headline = t("正在处理", "Working");
    s.accent = "blue".into();
}

fn steps_text(steps: i64) -> String {
    if english() { format!(" · {steps} steps") } else { format!(" · {steps} 步") }
}

fn hold_answer(s: &mut Session, now: Stamp) {
    if s.stop_seen || s.work_outstanding() {
        return;
    }
    s.phase = "settling".into();
    s.done_until = None;
    s.headline = t("回复写好了", "Reply is ready");
    s.accent = "green".into();
    s.activity = t("可以先看对话", "You can read it now");
    s.pose = "done".into();
    if !s.snippet.is_empty() {
        s.detail = s.snippet.clone();
    }
    s.settle_until = Some(now.add_secs(8.0));
    if s.started_at.is_some() {
        s.summary = t("已用 ", "") + &elapsed(s.started_at, now) + &steps_text(s.steps);
    }
    add_history(s, &t("回复写好了", "Reply is ready"));
}

fn update_answer_state(s: &mut Session, now: Stamp) {
    if s.answer_seen {
        hold_answer(s, now);
    }
}

fn finished_summary(s: &Session, now: Stamp) -> String {
    t("共用 ", "Took ") + &elapsed(s.started_at, now) + &steps_text(s.steps)
}

fn complete_turn(s: &mut Session, status: &str, now: Stamp, others_running: bool, done_seconds: i64) {
    let first_done = s.phase != "done";
    s.stop_seen = true;
    s.phase = "done".into();
    s.settle_until = None;
    s.open_tools = 0;
    s.open_subagents = 0;
    s.open_tool_ids.clear();
    let (headline, accent, pose) = match status {
        "aborted" => (t("这一轮已中断", "Turn stopped"), "orange", "stop"),
        "error" => (t("这一轮出错了", "Turn failed"), "red", "error"),
        _ => (t("回答结束了", "All done"), "green", "done"),
    };
    s.headline = headline;
    s.accent = accent.into();
    s.pose = pose.into();
    s.done_at = Some(now);
    if others_running {
        s.spend_shared = true;
    }
    s.summary = finished_summary(s, now);
    s.activity = if !s.project.is_empty() { s.project.clone() } else { t("可以回到对话里看", "Back to the chat for details") };
    if !s.snippet.is_empty() {
        s.detail = s.snippet.clone();
    }
    if first_done {
        s.done_until = Some(now.add_secs(done_seconds as f64));
        s.sound_pending = true;
    }
    let generation = s.generation_id.clone();
    add_retired(s, &generation);
    let headline = s.headline.clone();
    add_history(s, &headline);
}

fn expire_stale_turn(s: &mut Session, now: Stamp, done_seconds: i64) {
    if s.phase == "done" || s.phase == "idle" {
        return;
    }
    s.stop_seen = true;
    s.phase = "done".into();
    s.settle_until = None;
    s.open_tools = 0;
    s.open_subagents = 0;
    s.open_tool_ids.clear();
    s.headline = t("会话失去连接", "Session went quiet");
    s.activity = t("长时间没有收到 Cursor 事件", "No Cursor events for a long time");
    s.detail.clear();
    s.accent = "orange".into();
    s.pose = "sleep".into();
    s.summary = t("最后活动于 ", "Last activity ") + &s.updated_at.local_hm();
    s.done_until = Some(now.add_secs(done_seconds as f64));
    s.sound_pending = false;
    let generation = s.generation_id.clone();
    add_retired(s, &generation);
    let headline = s.headline.clone();
    add_history(s, &headline);
}

fn soft_complete(s: &mut Session, now: Stamp, others_running: bool, done_seconds: i64) {
    if s.stop_seen || s.phase == "done" {
        return;
    }
    s.phase = "done".into();
    s.settle_until = None;
    s.headline = t("回答结束了", "All done");
    s.accent = "green".into();
    s.pose = "done".into();
    s.summary = finished_summary(s, now);
    s.done_at = Some(now);
    if others_running {
        s.spend_shared = true;
    }
    if !s.project.is_empty() {
        s.activity = s.project.clone();
    } else if s.activity.trim().is_empty() {
        s.activity = t("可以回到对话里看", "Back to the chat for details");
    }
    if !s.snippet.is_empty() {
        s.detail = s.snippet.clone();
    }
    s.done_until = Some(now.add_secs(done_seconds as f64));
    s.sound_pending = true;
    let headline = s.headline.clone();
    add_history(s, &headline);
}

pub fn tool_pose(tool: &str) -> &'static str {
    match tool {
        t if t.starts_with("MCP:") => "web",
        "Read" | "TabRead" => "read",
        "Write" | "StrReplace" | "Edit" | "EditNotebook" | "Delete" => "write",
        "Shell" => "terminal",
        "Grep" | "Glob" | "SemanticSearch" => "search",
        "WebSearch" | "WebFetch" => "web",
        "Task" => "delegate",
        _ => "think",
    }
}

pub fn tool_phrase(tool: &str, finished: bool) -> String {
    let (zh, en): (String, String) = match tool {
        t if t.starts_with("MCP:") => ("调用外部工具".into(), "calling an external tool".into()),
        "Read" | "TabRead" => ("读取".into(), "reading".into()),
        "Write" | "StrReplace" | "Edit" | "EditNotebook" | "Delete" => ("修改".into(), "editing".into()),
        "Shell" => ("执行命令".into(), "running a command".into()),
        "Grep" | "Glob" | "SemanticSearch" => ("搜索".into(), "searching".into()),
        "Task" => ("派生子任务".into(), "starting a subtask".into()),
        "WebSearch" | "WebFetch" => ("查找资料".into(), "looking things up".into()),
        t if t.trim().is_empty() => ("处理".into(), "working".into()),
        t => (format!("使用 {t}"), format!("using {t}")),
    };
    if english() {
        let phrase = if finished { format!("Done {en}") } else { en };
        let mut chars = phrase.chars();
        return match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => phrase,
        };
    }
    format!("{}{zh}", if finished { "已完成" } else { "正在" })
}

fn property_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                return match serde_json::from_str::<Value>(trimmed) {
                    Ok(parsed) => property_text(Some(&parsed)),
                    Err(_) => shorten(text, 80),
                };
            }
            shorten(text, 80)
        }
        Some(Value::Object(map)) => {
            for key in DETAIL_KEYS {
                match map.get(key) {
                    Some(Value::String(s)) if !s.is_empty() => return s.clone(),
                    Some(Value::Number(n)) => return n.to_string(),
                    Some(Value::Bool(b)) => return b.to_string(),
                    _ => {}
                }
            }
            String::new()
        }
        _ => String::new(),
    }
}

fn tool_detail(e: &HookEvent) -> String {
    let mut direct = property_text(e.value("tool_input"));
    for key in ["agent_message", "file_path", "command", "task"] {
        if !direct.trim().is_empty() {
            break;
        }
        direct = e.get(key);
    }
    let b = direct.as_bytes();
    let drive_path = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    if drive_path || direct.starts_with('/') {
        let name = leaf(&direct);
        if !name.trim().is_empty() {
            direct = name;
        }
    }
    shorten(&direct, 72)
}

fn apply_to(s: &mut Session, e: &HookEvent, event_name: &str, now: Stamp, others_running: bool, done_seconds: i64) {
    let roots = e.workspace_roots();
    if !roots.is_empty() {
        s.project = roots.iter().map(|r| leaf(r)).collect::<Vec<_>>().join("\u{3001}");
        s.workspace_root = roots[0].clone();
    }

    let generation = e.get("generation_id");
    if !generation.trim().is_empty() && s.retired_generations.contains(&generation) {
        return;
    }
    if event_name == "beforeSubmitPrompt" {
        if !generation.is_empty() && s.generation_id == generation && s.started_at.is_some() && !s.stop_seen {
            return;
        }
        if !generation.is_empty() && !s.generation_id.is_empty() && generation != s.generation_id {
            let old = s.generation_id.clone();
            add_retired(s, &old);
        }
        reset_turn(s, now, others_running);
        if !generation.is_empty() {
            s.generation_id = generation.clone();
        }
    } else {
        if !generation.is_empty() && !s.generation_id.is_empty() && generation != s.generation_id {
            return;
        }
        if s.started_at.is_none() {
            reset_turn(s, now, others_running);
        }
        if !generation.is_empty() && s.generation_id.is_empty() {
            s.generation_id = generation.clone();
        }
    }

    // stop ends this generation; a late response or thought must not restart it
    if s.stop_seen {
        return;
    }

    let tool_name = e.get("tool_name");
    let tool_use_id = e.get("tool_use_id");
    match event_name {
        "beforeSubmitPrompt" => {
            s.prompt = shorten(&e.get("prompt"), 60);
            if let Ok(window) = e.get("host_window").parse::<i64>() {
                if window != 0 {
                    s.host_window = window;
                    s.host_process = e.get("host_process");
                }
            }
            s.headline = t("正在处理", "Working");
            s.activity = t("已收到问题", "Got your question");
            s.detail = s.prompt.clone();
            s.accent = "blue".into();
            s.pose = "think".into();
            let activity = s.activity.clone();
            add_history(s, &activity);
        }
        "preToolUse" => {
            if !tool_use_id.is_empty() {
                if s.completed_tool_ids.contains(&tool_use_id) || s.open_tool_ids.contains(&tool_use_id) {
                    return;
                }
                s.open_tool_ids.insert(tool_use_id.clone());
                s.open_tools = s.open_tool_ids.len() as i64;
            } else {
                s.open_tools += 1;
            }
            s.answer_seen = false;
            enter_running(s);
            s.steps += 1;
            s.step_started_at = Some(now);
            s.pose = tool_pose(&tool_name).into();
            s.activity = tool_phrase(&tool_name, false);
            s.detail = tool_detail(e);
            let line = if s.detail.is_empty() { s.activity.clone() } else { format!("{} {}", s.activity, s.detail) };
            add_history(s, &line);
        }
        "postToolUse" => {
            let mut had_open_tool = false;
            if !tool_use_id.is_empty() {
                if s.completed_tool_ids.contains(&tool_use_id) {
                    return;
                }
                had_open_tool = s.open_tool_ids.remove(&tool_use_id);
                s.completed_tool_ids.insert(tool_use_id.clone());
                s.open_tools = s.open_tool_ids.len() as i64;
            } else if s.open_tools > 0 {
                had_open_tool = true;
                s.open_tools -= 1;
            }
            if !had_open_tool {
                enter_running(s);
                s.steps += 1;
            }
            s.step_started_at = Some(now);
            if !tool_name.is_empty() {
                s.pose = tool_pose(&tool_name).into();
                s.activity = tool_phrase(&tool_name, true);
            }
            let detail = tool_detail(e);
            if !detail.is_empty() {
                s.detail = detail;
            }
            if !had_open_tool {
                let line = if s.detail.is_empty() { s.activity.clone() } else { format!("{} {}", s.activity, s.detail) };
                add_history(s, &line);
            }
            update_answer_state(s, now);
        }
        "postToolUseFailure" => {
            let mut had_open_tool = false;
            if !tool_use_id.is_empty() {
                if s.completed_tool_ids.contains(&tool_use_id) {
                    return;
                }
                had_open_tool = s.open_tool_ids.remove(&tool_use_id);
                s.completed_tool_ids.insert(tool_use_id.clone());
                s.open_tools = s.open_tool_ids.len() as i64;
            } else if s.open_tools > 0 {
                had_open_tool = true;
                s.open_tools -= 1;
            }
            enter_running(s);
            if !had_open_tool {
                s.steps += 1;
            }
            s.accent = "orange".into();
            s.pose = "error".into();
            s.activity = t("这一步没有成功", "This step failed");
            s.detail = shorten(&e.get("error_message"), 72);
            s.step_started_at = Some(now);
            let line = format!("{} {}", s.activity, s.detail);
            add_history(s, &line);
        }
        "afterAgentThought" => {
            if s.answer_seen {
                update_answer_state(s, now);
                return;
            }
            enter_running(s);
            s.steps += 1;
            s.step_started_at = Some(now);
            s.pose = "think".into();
            s.activity = t("正在思考", "Thinking");
            s.detail.clear();
            let activity = s.activity.clone();
            add_history(s, &activity);
        }
        "afterAgentResponse" => {
            s.answer_seen = true;
            let snippet = shorten(&e.get("text"), 140);
            if !snippet.is_empty() {
                s.snippet = snippet;
            }
            if s.work_outstanding() {
                s.activity = t("正在收尾", "Wrapping up");
                if !s.snippet.is_empty() {
                    s.detail = s.snippet.clone();
                }
            }
            update_answer_state(s, now);
            add_history(s, &t("回复写好了", "Reply is ready"));
        }
        "subagentStart" => {
            s.answer_seen = false;
            enter_running(s);
            s.steps += 1;
            s.step_started_at = Some(now);
            s.pose = "delegate".into();
            s.activity = t("正在处理子任务", "Running a subtask");
            s.detail = shorten(&e.get("task"), 72);
            let line = format!("{} {}", s.activity, s.detail);
            add_history(s, &line);
        }
        "subagentStop" => {
            s.step_started_at = Some(now);
            if s.answer_seen && !s.work_outstanding() {
                update_answer_state(s, now);
            } else {
                s.phase = "running".into();
                s.settle_until = None;
                s.done_until = None;
                s.headline = t("正在处理", "Working");
                s.activity = t("子任务已返回，继续处理", "Subtask returned, carrying on");
                let activity = s.activity.clone();
                add_history(s, &activity);
            }
        }
        "stop" => {
            let status = e.get("status");
            let status = if status.is_empty() { "completed".to_string() } else { status };
            complete_turn(s, &status, now, others_running, done_seconds);
        }
        _ => {
            if !s.answer_seen {
                enter_running(s);
                s.activity = t("正在处理", "Working");
                s.step_started_at = Some(now);
            }
        }
    }
}
