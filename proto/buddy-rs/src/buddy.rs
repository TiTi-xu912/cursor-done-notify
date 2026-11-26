use crate::clock::Stamp;
use crate::event::HookEvent;
use crate::quota::{self, QuotaRow, QuotaService, QuotaSnapshot};
use crate::render::{self, Palette, Spot};
use crate::sprites::{self, Clip};
use crate::state::{BuddyState, Session, Settings, View};
use crate::text::Fonts;
use crate::update::UpdateInfo;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;

pub enum Click {
    Nothing,
    Dismiss,
    Focus(Session),
}

pub struct Tick {
    pub repaint: bool,
    pub close: bool,
}

pub struct Buddy {
    pub state: BuddyState,
    pub settings_path: PathBuf,
    pub dark: bool,
    fonts: Fonts,
    pose_dir: PathBuf,
    state_dir: PathBuf,
    inbox_dir: PathBuf,
    clip: Option<Clip>,
    frame: usize,
    frame_started: Instant,
    sprite_aspect: f32,
    scale: f32,
    canvas: Pixmap,
    spot: Spot,
    view: View,
    expanded: bool,
    started: Instant,
    last_scan: Option<Instant>,
    poke_until: Option<Instant>,
    live_view: String,
    dirty: bool,
    quota: Option<QuotaService>,
    /// Stands in for the service when rendering snapshots.
    quota_fixed: Option<QuotaSnapshot>,
    quota_seen: Option<u32>,
    quota_expanded: bool,
    quota_rows: Vec<QuotaRow>,
    last_phase: String,
    pub update: Option<UpdateInfo>,
    update_inbox: Arc<Mutex<Option<UpdateInfo>>>,
    update_balloon: Option<String>,
}

impl Buddy {
    pub fn new(scale: f32, state_dir: &Path, settings_path: PathBuf) -> Result<Buddy, String> {
        let fonts = Fonts::load().ok_or("找不到中文字体")?;
        let pose_dir = sprites::find_pose_dir().ok_or("找不到 sprites/poses 目录")?;
        let probe = sprites::load(&pose_dir, "idle", 64).ok_or("读不了 idle 精灵图")?;
        let sprite_aspect = probe.frames[0].width() as f32 / probe.frames[0].height() as f32;
        drop(probe);

        let settings = Settings::load(&settings_path);
        crate::lang::apply(&settings.language);
        let mut state = BuddyState::new();
        state.apply_timing(&settings);
        state.settings = settings;
        state.quota_alert_path = Some(state_dir.join("quota-alert.txt"));

        let view = state.presentation(Stamp::now());
        let spot = render::layout(&view, false, &[], &fonts, scale, sprite_aspect);
        let canvas = Pixmap::new(spot.width, spot.height).ok_or("画布尺寸无效")?;
        let now = Instant::now();
        Ok(Buddy {
            state,
            settings_path,
            dark: false,
            fonts,
            pose_dir,
            state_dir: state_dir.to_path_buf(),
            inbox_dir: state_dir.join("inbox"),
            clip: None,
            frame: 0,
            frame_started: now,
            sprite_aspect,
            scale,
            canvas,
            spot,
            view,
            expanded: false,
            started: now,
            last_scan: None,
            poke_until: None,
            live_view: String::new(),
            dirty: true,
            quota: None,
            quota_fixed: None,
            quota_seen: None,
            quota_expanded: false,
            quota_rows: Vec::new(),
            last_phase: String::new(),
            update: None,
            update_inbox: Arc::new(Mutex::new(None)),
            update_balloon: None,
        })
    }

    /// Starts reading Cursor usage unless CURSOR_NOTIFY_QUOTA=0 turns it off for good.
    pub fn start_quota(&mut self) {
        if std::env::var("CURSOR_NOTIFY_QUOTA").as_deref() == Ok("0") {
            return;
        }
        self.quota = Some(QuotaService::start(quota::default_database(), self.state_dir.join("quota-cache.txt")));
    }

    pub fn has_quota(&self) -> bool {
        self.quota.is_some()
    }

    fn quota_latest(&self) -> Option<QuotaSnapshot> {
        self.quota.as_ref().and_then(|q| q.latest()).or_else(|| self.quota_fixed.clone())
    }

    /// At most one GitHub check a day; the answer arrives through `update_inbox`.
    pub fn start_update_check(&mut self) {
        if !self.state.settings.updates || std::env::var("CURSOR_NOTIFY_UPDATES").as_deref() == Ok("0") {
            return;
        }
        let path = self.state_dir.join("update-check.txt");
        let cached = UpdateInfo::load(&path);
        if let Some(c) = cached.as_ref().filter(|c| c.checked.is_some_and(|at| Stamp::now().secs_since(at) < 86_400.0)) {
            self.accept_update(c.clone());
            return;
        }
        let inbox = self.update_inbox.clone();
        let _ = std::thread::Builder::new().name("update".into()).stack_size(256 * 1024).spawn(move || {
            let fresh = UpdateInfo::fetch(cached.as_ref());
            *inbox.lock().unwrap() = Some(fresh);
        });
    }

    fn accept_update(&mut self, mut info: UpdateInfo) {
        if info.newer() && info.notified_tag != info.tag {
            info.notified_tag = info.tag.clone();
            self.update_balloon = Some(info.tag.clone());
        }
        info.save(&self.state_dir.join("update-check.txt"));
        self.update = Some(info);
    }

    /// The tag to announce once in a tray balloon, if a new release just showed up.
    pub fn take_update_balloon(&mut self) -> Option<String> {
        self.update_balloon.take()
    }

    pub fn request_quota_refresh(&self) {
        if let Some(q) = &self.quota {
            q.request_refresh(false);
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.spot.width, self.spot.height)
    }

    pub fn canvas(&self) -> &Pixmap {
        &self.canvas
    }

    pub fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() > 0.01 {
            self.scale = scale;
            self.clip = None;
            self.relayout();
        }
    }

    pub fn save_settings(&mut self) {
        self.state.settings.save(&self.settings_path);
        crate::lang::apply(&self.state.settings.language);
        let settings = self.state.settings.clone();
        self.state.apply_timing(&settings);
        self.view = View::default();
        self.dirty = true;
    }

    /// Removes leftovers older than a day; run once at startup.
    pub fn clear_stale(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.inbox_dir) {
            let cutoff = std::time::SystemTime::now() - Duration::from_secs(86_400);
            for entry in entries.flatten() {
                let old = entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < cutoff);
                if old {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let _ = std::fs::remove_file(self.state_dir.join("live-view.txt"));
    }

    fn scan_inbox(&mut self, now: Stamp) {
        let Ok(entries) = std::fs::read_dir(&self.inbox_dir) else { return };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        // Names start with zero-padded UTC ticks, so name order is arrival order.
        files.sort();
        for path in files {
            let Ok(raw) = std::fs::read_to_string(&path) else { continue };
            if !raw.trim().is_empty() {
                self.state.apply(&HookEvent::parse(&raw), now);
            }
            let _ = std::fs::remove_file(&path);
            self.dirty = true;
        }
    }

    /// `draw` is false while the window is hidden between turns.
    pub fn tick(&mut self, now: Instant, draw: bool) -> Tick {
        let stamp = Stamp::now();
        if self.last_scan.is_none_or(|t| now.duration_since(t) >= Duration::from_millis(150)) {
            self.last_scan = Some(now);
            self.scan_inbox(stamp);
        }
        self.state.update_lifecycle(stamp);
        self.state.play_pending_sounds(stamp);
        let idle_too_long = self.state.order.is_empty() && now.duration_since(self.started) > Duration::from_secs(60);
        if self.state.should_close(stamp) || idle_too_long {
            return Tick { repaint: false, close: true };
        }

        let fresh_update = self.update_inbox.lock().unwrap().take();
        if let Some(info) = fresh_update {
            self.accept_update(info);
        }
        self.tick_quota(stamp);

        let mut view = self.state.presentation(stamp);
        if let Some(q) = &self.quota {
            if view.phase == "done" && self.last_phase != "done" {
                q.request_refresh(false);
            }
        }
        self.last_phase = view.phase.clone();
        let notice = self.state.active_quota_notice(stamp);
        if !notice.is_empty() {
            view.hint = notice;
            view.hint_alert = true;
        }
        self.write_live_view();
        if !draw {
            return Tick { repaint: false, close: false };
        }
        let rows = if self.state.settings.quota && (self.quota.is_some() || self.quota_fixed.is_some()) {
            quota::rows(self.quota_latest().as_ref(), self.quota_expanded, stamp)
        } else {
            Vec::new()
        };
        if view != self.view || rows != self.quota_rows {
            self.view = view;
            self.quota_rows = rows;
            self.relayout();
        }

        let pose = if self.poke_until.is_some_and(|u| now < u) { "idle".to_string() } else { self.view.pose.clone() };
        self.ensure_clip(&pose, now);
        if let Some(clip) = &self.clip {
            let delay = Duration::from_millis(clip.delays_ms[self.frame] as u64);
            if now.duration_since(self.frame_started) >= delay {
                self.frame = (self.frame + 1) % clip.frames.len();
                self.frame_started = now;
                self.dirty = true;
            }
        }

        let repaint = std::mem::take(&mut self.dirty);
        if repaint {
            self.paint(now);
        }
        Tick { repaint, close: false }
    }

    /// Shows the next turn without waiting for a new process (used with --stay).
    pub fn reset_after_close(&mut self) {
        let keys = self.state.order.clone();
        for key in keys {
            self.state.remove_session(&key);
        }
        self.started = Instant::now();
        self.view = View::default();
        self.dirty = true;
    }

    pub fn has_sessions(&self) -> bool {
        !self.state.order.is_empty()
    }

    /// Mirrors the C# host: feed each new snapshot to the turn-spend and alert logic,
    /// and ask for samples around turn boundaries.
    fn tick_quota(&mut self, stamp: Stamp) {
        let Some(q) = &self.quota else { return };
        q.sync_language();
        let version = q.version();
        let latest = q.latest();
        if self.quota_seen != Some(version) {
            self.quota_seen = Some(version);
            if let Some(snap) = &latest {
                self.state.update_turn_spend(snap);
                if self.state.settings.quota {
                    self.state.update_quota_alert(snap, stamp);
                }
            }
        }
        if self.state.needs_spend_follow_up(latest.as_ref(), stamp) {
            q.request_refresh(true);
        } else if self.state.needs_spend_sample() {
            q.request_refresh(false);
        }
    }

    fn relayout(&mut self) {
        self.spot = render::layout(&self.view, self.expanded, &self.quota_rows, &self.fonts, self.scale, self.sprite_aspect);
        if self.canvas.width() != self.spot.width || self.canvas.height() != self.spot.height {
            if let Some(canvas) = Pixmap::new(self.spot.width, self.spot.height) {
                self.canvas = canvas;
            }
        }
        self.dirty = true;
    }

    fn ensure_clip(&mut self, pose: &str, now: Instant) {
        if self.clip.as_ref().is_some_and(|c| c.pose == pose) {
            return;
        }
        // Drop the old frames first so two poses are never resident together.
        self.clip = None;
        let height = (188.0 * self.scale).round() as u32;
        self.clip = sprites::load(&self.pose_dir, pose, height).or_else(|| sprites::load(&self.pose_dir, "idle", height));
        self.frame = 0;
        self.frame_started = now;
        self.dirty = true;
    }

    fn paint(&mut self, now: Instant) {
        let palette: &Palette = if self.dark { &render::DARK } else { &render::LIGHT };
        let hop = match self.poke_until {
            Some(until) if now < until => {
                let left = until.duration_since(now).as_secs_f32() / 0.52;
                -(left * std::f32::consts::PI).sin().abs() * 10.0 * self.scale
            }
            _ => 0.0,
        };
        let frame = self.clip.as_ref().map(|c| &c.frames[self.frame]);
        render::draw(&mut self.canvas, &self.spot, &self.view, frame, &self.fonts, palette, hop);
    }

    fn write_live_view(&mut self) {
        let live = self.state.live();
        let line = format!("{}|{}|{}|{}|", live.phase, live.steps, live.headline, live.activity);
        if line != self.live_view {
            let _ = std::fs::write(self.state_dir.join("live-view.txt"), &line);
            self.live_view = line;
        }
    }

    pub fn hit_hand(&self, x: f32, y: f32) -> bool {
        let s = &self.spot;
        s.close.contains(x, y)
            || s.quota.is_some_and(|a| a.contains(x, y))
            || s.rows.iter().any(|(_, a)| a.contains(x, y))
            || self.character_hit(x, y)
            || (self.can_focus() && s.headline.contains(x, y))
    }

    fn can_focus(&self) -> bool {
        self.state.order.len() <= 1 && BuddyState::can_jump(self.state.current.as_ref().and_then(|k| self.state.sessions.get(k)))
    }

    fn character_hit(&self, x: f32, y: f32) -> bool {
        if !self.spot.character.contains(x, y) {
            return false;
        }
        let (px, py) = (x as i32, y as i32);
        if px < 0 || py < 0 || px >= self.canvas.width() as i32 || py >= self.canvas.height() as i32 {
            return false;
        }
        self.canvas.pixel(px as u32, py as u32).is_some_and(|p| p.alpha() >= 16)
    }

    pub fn click(&mut self, x: f32, y: f32) -> Click {
        if self.spot.close.contains(x, y) {
            return Click::Dismiss;
        }
        if self.spot.quota.is_some_and(|a| a.contains(x, y)) {
            self.quota_expanded = !self.quota_expanded;
            if self.quota_expanded {
                self.request_quota_refresh();
            }
            self.quota_rows = quota::rows(self.quota_latest().as_ref(), self.quota_expanded, Stamp::now());
            self.relayout();
            return Click::Nothing;
        }
        if let Some((key, _)) = self.spot.rows.iter().find(|(_, a)| a.contains(x, y)).cloned() {
            let target = self.state.click_session(&key);
            self.view = View::default();
            self.dirty = true;
            if self.state.order.is_empty() {
                return Click::Dismiss;
            }
            return target.map_or(Click::Nothing, Click::Focus);
        }
        if self.character_hit(x, y) {
            self.poke_until = Some(Instant::now() + Duration::from_millis(520));
            self.dirty = true;
            return Click::Nothing;
        }
        if self.can_focus() && self.spot.headline.contains(x, y) {
            return self.state.current.as_ref().and_then(|k| self.state.sessions.get(k)).cloned().map_or(Click::Nothing, Click::Focus);
        }
        if self.view.phase == "done" || self.view.phase == "settling" {
            return Click::Dismiss;
        }
        self.expanded = !self.expanded;
        self.relayout();
        Click::Nothing
    }
}

/// Renders a few scripted turns through the real state machine, for checking the drawing.
pub fn snapshots(dir: &Path) -> Result<(), String> {
    let _ = std::fs::create_dir_all(dir);
    let scratch = std::env::temp_dir().join("cursor-done-notify-snapshot");
    let mut buddy = Buddy::new(1.0, &scratch, scratch.join("settings.txt"))?;
    let now = Stamp::now();
    let ev = |pairs: &[&str]| {
        let mut map = serde_json::Map::new();
        for pair in pairs.chunks_exact(2) {
            let value = if pair[0] == "workspace_roots" {
                serde_json::Value::Array(vec![pair[1].into()])
            } else {
                pair[1].into()
            };
            map.insert(pair[0].to_string(), value);
        }
        HookEvent::from_map(map)
    };
    let usage = r#"{"billingCycleStart":"1793770164000","billingCycleEnd":"1796362164000","planUsage":{"totalSpend":18888,"includedSpend":7000,"bonusSpend":11888,"limit":7000,"autoPercentUsed":13.06,"apiPercentUsed":29.2,"totalPercentUsed":14.4},"spendLimitUsage":{"individualLimit":2000,"individualUsed":550}}"#;
    buddy.quota_fixed = Some(quota::parse(usage, r#"{"planInfo":{"planName":"Pro+"}}"#, "pro_plus", now));
    let shot = |buddy: &mut Buddy, name: &str, expanded: bool| {
        buddy.expanded = expanded;
        buddy.quota_expanded = name.starts_with("quota");
        buddy.view = buddy.state.presentation(now.add_secs(65.0));
        buddy.quota_rows = quota::rows(buddy.quota_fixed.as_ref(), buddy.quota_expanded, now);
        buddy.relayout();
        let pose = buddy.view.pose.clone();
        buddy.ensure_clip(&pose, Instant::now());
        buddy.paint(Instant::now());
        let _ = buddy.canvas.save_png(dir.join(format!("{name}.png")));
    };
    let a = ["conversation_id", "a", "generation_id", "ga"];
    fn joined<'a>(head: &[&'a str], mid: &[&'a str], tail: &[&'a str]) -> Vec<&'a str> {
        [head, mid, tail].concat()
    }
    let with = |head: &[&'static str], tail: &[&'static str]| joined(head, &a, tail);
    buddy.state.apply(&ev(&with(&["hook_event_name", "beforeSubmitPrompt"], &["prompt", "把状态机搬到 Rust", "workspace_roots", r"E:\n\cursor-done-notify"])), now);
    buddy.state.apply(&ev(&with(&["hook_event_name", "postToolUse"], &["tool_name", "Read", "tool_use_id", "1", "file_path", r"E:\n\cursor-done-notify\Host.cs"])), now);
    shot(&mut buddy, "read", false);
    buddy.state.apply(&ev(&with(&["hook_event_name", "preToolUse"], &["tool_name", "Shell", "tool_use_id", "2", "command", "cargo test"])), now);
    shot(&mut buddy, "terminal-steps", true);
    buddy.state.apply(&ev(&with(&["hook_event_name", "postToolUse"], &["tool_name", "Shell", "tool_use_id", "2"])), now);
    buddy.state.apply(&ev(&with(&["hook_event_name", "afterAgentResponse"], &["text", "状态机已经搬完，11 组测试全部通过。接下来接上收件箱，让 Rust 宿主跟着真实对话跑。"])), now);
    buddy.state.apply(&ev(&with(&["hook_event_name", "stop"], &["status", "completed"])), now.add_secs(64.0));
    shot(&mut buddy, "done", false);
    shot(&mut buddy, "quota", false);
    let b = ["conversation_id", "b", "generation_id", "gb"];
    buddy.state.apply(&ev(&[&["hook_event_name", "beforeSubmitPrompt"][..], &b, &["prompt", "看一下额度栏"]].concat()), now);
    buddy.state.apply(&ev(&[&["hook_event_name", "preToolUse"][..], &b, &["tool_name", "Grep", "tool_use_id", "3", "pattern", "QuotaSnapshot"]].concat()), now);
    let c = ["conversation_id", "c", "generation_id", "gc"];
    buddy.state.apply(&ev(&[&["hook_event_name", "beforeSubmitPrompt"][..], &c, &["prompt", "修一下安装脚本"]].concat()), now);
    buddy.state.apply(&ev(&[&["hook_event_name", "stop"][..], &c, &["status", "error"]].concat()), now);
    shot(&mut buddy, "board", false);
    buddy.dark = true;
    shot(&mut buddy, "board-dark", false);
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}
