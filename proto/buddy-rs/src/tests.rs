// Ported one-to-one from SelfTest in HostApp.cs; keep the two in step.
use crate::clock::{elapsed, Stamp};
use crate::event::HookEvent;
use crate::lang;
use crate::state::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn ev(pairs: &[&str]) -> HookEvent {
    HookEvent::from_pairs(pairs)
}

fn fresh() -> BuddyState {
    lang::set_english(false);
    let mut st = BuddyState::new();
    st.done_seconds = 12;
    st.stale_minutes = 180;
    st.sound = Box::new(|_| {});
    st.foreground = Box::new(|| Some(Foreground { process: String::new(), title: String::new(), window: "0".into() }));
    st
}

fn foreground(process: &str, title: &str, window: &str) -> Box<dyn Fn() -> Option<Foreground>> {
    let fg = Foreground { process: process.into(), title: title.into(), window: window.into() };
    Box::new(move || Some(fg.clone()))
}

#[track_caller]
fn phase(st: &BuddyState, name: &str, expected: &str) {
    let s = st.live();
    assert_eq!(
        s.phase, expected,
        "{name}: stop={} answer={} tools={}",
        s.stop_seen, s.answer_seen, s.open_tools
    );
}

#[test]
fn lifecycle() {
    let now = Stamp::now();
    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "generation_id", "g1", "prompt", "你好", "workspace_roots", r"D:\demo"]), now);
    phase(&st, "prompt", "running");
    st.apply(&ev(&["hook_event_name", "preToolUse", "generation_id", "g1", "tool_name", "Read", "file_path", "a.txt"]), now);
    phase(&st, "tool", "running");
    assert_eq!(st.live().open_tools, 1, "open tools");
    st.apply(&ev(&["hook_event_name", "postToolUse", "generation_id", "g1", "tool_name", "Read", "file_path", "a.txt"]), now);
    phase(&st, "tool done", "running");
    assert_eq!(st.live().open_tools, 0, "tools not closed");
    st.apply(&ev(&["hook_event_name", "afterAgentResponse", "generation_id", "g1", "text", "已经好了"]), now);
    phase(&st, "answer", "settling");
    st.apply(&ev(&["hook_event_name", "afterAgentThought", "generation_id", "g1", "text", "late"]), now);
    phase(&st, "late thought", "settling");
    st.apply(&ev(&["hook_event_name", "stop", "generation_id", "g1", "status", "completed"]), now);
    phase(&st, "stop", "done");
    st.apply(&ev(&["hook_event_name", "afterAgentResponse", "generation_id", "g1", "text", "late answer"]), now);
    phase(&st, "late answer", "done");
    st.apply(&ev(&["hook_event_name", "preToolUse", "generation_id", "g1", "tool_name", "Shell"]), now);
    phase(&st, "late tool", "done");

    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "generation_id", "g2", "prompt", "下一问"]), now);
    phase(&st, "new turn", "running");
    st.apply(&ev(&["hook_event_name", "preToolUse", "generation_id", "g2", "tool_name", "Read"]), now);
    st.apply(&ev(&["hook_event_name", "afterAgentResponse", "generation_id", "g2", "text", "先说一句"]), now);
    phase(&st, "answer while tool open", "running");
    st.apply(&ev(&["hook_event_name", "postToolUse", "generation_id", "g2", "tool_name", "Read"]), now);
    phase(&st, "tool closed after answer", "settling");
    st.apply(&ev(&["hook_event_name", "preToolUse", "generation_id", "g2", "tool_name", "Write"]), now);
    phase(&st, "work resumed", "running");
    st.apply(&ev(&["hook_event_name", "postToolUse", "generation_id", "g2", "tool_name", "Write"]), now);
    st.apply(&ev(&["hook_event_name", "afterAgentResponse", "generation_id", "g2", "text", "最终回复"]), now);
    phase(&st, "final answer", "settling");
    st.live_mut().settle_until = Some(now.add_secs(-1.0));
    st.update_lifecycle(now);
    phase(&st, "soft done", "done");
    assert!(!st.live().stop_seen, "soft done must not lock the generation");
    st.apply(&ev(&["hook_event_name", "preToolUse", "generation_id", "g2", "tool_name", "Grep"]), now);
    phase(&st, "resume after soft done", "running");
    assert!(!st.live().sound_pending, "resumed work kept a pending completion sound");

    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "stop", "generation_id", "g3", "status", "completed"]), now);
    st.apply(&ev(&["hook_event_name", "afterAgentResponse", "generation_id", "g3", "text", "out of order"]), now);
    phase(&st, "stop before response", "done");

    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "stale", "generation_id", "stale-g", "prompt", "失联测试"]), now);
    let stale = st.stale_minutes as f64;
    st.live_mut().updated_at = now.add_mins(-(stale + 1.0));
    st.update_lifecycle(now);
    phase(&st, "stale session", "done");
    assert!(st.live().headline == "会话失去连接" && !st.live().sound_pending, "stale session did not expire quietly");
    assert_eq!(st.current_pose(st.live(), now), "sleep", "a stale session dozes");
}

#[test]
fn tool_ids() {
    let now = Stamp::now();
    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "generation_id", "tool-ids", "prompt", "工具去重"]), now);
    st.apply(&ev(&["hook_event_name", "preToolUse", "generation_id", "tool-ids", "tool_name", "Read", "tool_use_id", "t1"]), now);
    st.apply(&ev(&["hook_event_name", "preToolUse", "generation_id", "tool-ids", "tool_name", "Read", "tool_use_id", "t1"]), now);
    assert!(st.live().open_tools == 1 && st.live().steps == 1, "duplicate tool start was counted twice");
    st.apply(&ev(&["hook_event_name", "postToolUse", "generation_id", "tool-ids", "tool_name", "Read", "tool_use_id", "t1"]), now);
    st.apply(&ev(&["hook_event_name", "postToolUse", "generation_id", "tool-ids", "tool_name", "Read", "tool_use_id", "t1"]), now);
    assert!(st.live().open_tools == 0 && st.live().steps == 1, "duplicate tool stop changed tool state");

    let mut st = fresh();
    let c = ["conversation_id", "late-pre", "generation_id", "late-pre-g"];
    st.apply(&ev(&[&["hook_event_name", "beforeSubmitPrompt"][..], &c, &["prompt", "迟到事件"]].concat()), now);
    st.apply(&ev(&[&["hook_event_name", "preToolUse"][..], &c, &["tool_name", "Read", "tool_use_id", "late-t1"]].concat()), now);
    st.apply(&ev(&[&["hook_event_name", "afterAgentResponse"][..], &c, &["text", "回答已写好"]].concat()), now);
    st.apply(&ev(&[&["hook_event_name", "preToolUse"][..], &c, &["tool_name", "Read", "tool_use_id", "late-t1"]].concat()), now);
    assert!(st.live().answer_seen, "duplicate tool start cleared a completed answer");
    st.apply(&ev(&[&["hook_event_name", "postToolUse"][..], &c, &["tool_name", "Read", "tool_use_id", "late-t1"]].concat()), now);
    phase(&st, "duplicate start after answer", "settling");

    let mut st = fresh();
    let c = ["conversation_id", "safe", "generation_id", "safe-g"];
    st.apply(&ev(&[&["hook_event_name", "beforeSubmitPrompt"][..], &c, &["prompt", "安全模式"]].concat()), now);
    st.apply(&ev(&[&["hook_event_name", "postToolUseFailure"][..], &c, &["tool_name", "Shell", "tool_use_id", "failed", "error_message", "失败"]].concat()), now);
    assert!(st.live().accent == "orange" && st.live().steps == 1, "post-only failure was not counted");
    let success = [&["hook_event_name", "postToolUse"][..], &c, &["tool_name", "Read", "tool_use_id", "success", "file_path", "safe.txt"]].concat();
    st.apply(&ev(&success), now);
    phase(&st, "post-only success", "running");
    assert!(st.live().accent == "blue" && st.live().steps == 2 && st.live().pose == "read", "post-only success kept stale state");
    assert!(st.live().history.join("\n").contains("已完成读取"), "post-only success was not added to history");
    st.apply(&ev(&success), now);
    assert_eq!(st.live().steps, 2, "duplicate post-only success was counted twice");
}

#[test]
fn generations() {
    let now = Stamp::now();
    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "gen-chat", "generation_id", "old-gen", "prompt", "旧轮次"]), now);
    st.apply(&ev(&["hook_event_name", "stop", "conversation_id", "gen-chat", "generation_id", "old-gen", "status", "completed"]), now);
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "gen-chat", "generation_id", "new-gen", "prompt", "新轮次"]), now);
    st.apply(&ev(&["hook_event_name", "postToolUse", "conversation_id", "gen-chat", "generation_id", "old-gen", "tool_name", "Read"]), now);
    phase(&st, "old generation ignored", "running");
    assert_eq!(st.live().generation_id, "new-gen", "old generation replaced active generation");
}

#[test]
fn multi_session() {
    let now = Stamp::now();
    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "chat-a", "generation_id", "ga", "prompt", "修登录"]), now);
    st.apply(&ev(&["hook_event_name", "preToolUse", "conversation_id", "chat-a", "generation_id", "ga", "tool_name", "Read", "file_path", "login.cs"]), now);
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "chat-b", "generation_id", "gb", "prompt", "看报表"]), now);
    st.apply(&ev(&["hook_event_name", "stop", "conversation_id", "chat-b", "generation_id", "gb", "status", "error"]), now);
    phase(&st, "second chat failed", "done");
    assert_eq!(st.live().accent, "red", "second chat should be the error");
    st.apply(&ev(&["hook_event_name", "postToolUse", "conversation_id", "chat-a", "generation_id", "ga", "tool_name", "Read", "file_path", "login.cs"]), now);
    phase(&st, "first chat still running", "running");
    assert!(st.sessions["chat-b"].phase == "done" && st.sessions["chat-b"].accent == "red", "finished chat lost its error");
    let overview = st.presentation(now);
    assert!(overview.phase == "running" && overview.accent == "red", "overview with a running chat must stay running: {}/{}", overview.phase, overview.accent);
    let board = st.board_text();
    assert!(board.contains("bad|") && board.contains("run|"), "board missing a row: {board}");
    st.click_session("chat-b");
    assert!(!st.sessions.contains_key("chat-b"), "click should drop the finished chat");
    assert_eq!(st.sessions["chat-a"].phase, "running", "click changed the other chat");
    assert!(st.click_session("chat-a").is_some() && st.pinned_key == "chat-a", "click should pin the running chat");
    st.click_session("chat-a");
    assert!(st.pinned_key.is_empty(), "second click should return to the overview");
}

#[test]
fn poses() {
    let now = Stamp::now();
    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "poses", "generation_id", "poses-g", "prompt", "动作"]), now);
    st.apply(&ev(&["hook_event_name", "postToolUse", "conversation_id", "poses", "generation_id", "poses-g", "tool_name", "Task", "tool_use_id", "p1"]), now);
    assert_eq!(st.current_pose(st.live(), now), "delegate", "Task pose");
    st.apply(&ev(&["hook_event_name", "postToolUse", "conversation_id", "poses", "generation_id", "poses-g", "tool_name", "WebFetch", "tool_use_id", "p2"]), now);
    assert_eq!(st.current_pose(st.live(), now), "web", "WebFetch pose");
    let doze = st.doze_seconds as f64;
    st.live_mut().step_started_at = Some(now.add_secs(-(doze + 5.0)));
    assert_eq!(st.current_pose(st.live(), now), "sleep", "a quiet turn should doze");
    st.live_mut().open_tools = 1;
    assert_ne!(st.current_pose(st.live(), now), "sleep", "an open tool must not doze");
    st.live_mut().open_tools = 0;
    st.apply(&ev(&["hook_event_name", "stop", "conversation_id", "poses", "generation_id", "poses-g", "status", "aborted"]), now);
    assert_eq!(st.current_pose(st.live(), now), "stop", "aborted pose");
}

fn snap(spend: i64, fetched: Stamp) -> QuotaSnapshot {
    QuotaSnapshot { ok: true, total_spend_cents: Some(spend), fetched_at: Some(fetched), ..QuotaSnapshot::default() }
}

#[test]
fn spend() {
    let now = Stamp::now();
    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "spend", "generation_id", "spend-g", "prompt", "花费"]), now);
    st.update_turn_spend(&snap(1000, now));
    assert_eq!(st.live().spend_start, Some(1000), "spend baseline");
    assert!(!st.needs_spend_sample(), "baseline should be satisfied");
    st.update_turn_spend(&snap(1010, now));
    assert_eq!(st.live().spend_start, Some(1000), "baseline must not move while running");
    st.apply(&ev(&["hook_event_name", "stop", "conversation_id", "spend", "generation_id", "spend-g", "status", "completed"]), now);
    st.update_turn_spend(&snap(1042, now.add_secs(1.0)));
    let meta = st.meta_text(st.live(), now);
    assert!(meta.contains("本轮约 $0.42"), "turn spend: {meta}");
    let later = |secs: f64| QuotaSnapshot { fetched_at: Some(now.add_secs(secs)), ..QuotaSnapshot::default() };
    assert!(st.needs_spend_follow_up(Some(&later(1.0)), now), "a fresh finish should keep sampling spend");
    assert!(!st.needs_spend_follow_up(Some(&later(6.0)), now), "a late enough sample should stop the follow-up");
    st.live_mut().done_at = Some(now.add_secs(-20.0));
    assert!(!st.needs_spend_follow_up(None, now), "follow-up must give up after 15 seconds");

    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "stale-baseline", "generation_id", "sb", "prompt", "旧数据"]), now);
    st.update_turn_spend(&snap(500, now.add_mins(-10.0)));
    assert!(st.live().spend_start.is_none(), "an old cached snapshot must not become the baseline");
}

fn temp_file(prefix: &str) -> std::path::PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{n}-{:?}.txt", std::thread::current().id()))
}

#[test]
fn alerts() {
    let now = Stamp::now();
    let mut st = fresh();
    let sounds = Rc::new(Cell::new(0));
    let counter = sounds.clone();
    st.sound = Box::new(move |_| counter.set(counter.get() + 1));
    let path = temp_file("cursor-done-notify-alert");
    st.quota_alert_path = Some(path.clone());
    let cycle_end = now.add_secs(9.0 * 86_400.0);
    let q = |total: f64, api: f64, end: Stamp| QuotaSnapshot {
        ok: true,
        total_percent: Some(total),
        api_percent: Some(api),
        cycle_end: Some(end),
        ..QuotaSnapshot::default()
    };
    st.update_quota_alert(&q(60.0, 20.0, cycle_end), now);
    assert!(st.active_quota_notice(now).is_empty(), "no alert below 80%");
    st.update_quota_alert(&q(61.0, 85.6, cycle_end), now);
    assert_eq!(st.active_quota_notice(now), "本期 API 额度已用 85%", "80% alert");
    assert_eq!(sounds.get(), 1, "the alert should sound once");
    st.quota_notice.clear();
    st.update_quota_alert(&q(62.0, 88.0, cycle_end), now);
    assert!(st.active_quota_notice(now).is_empty(), "the same level must alert only once per cycle");
    st.update_quota_alert(&q(101.0, 90.0, cycle_end), now);
    assert_eq!(st.active_quota_notice(now), "本期额度已用完", "100% alert");
    st.quota_notice.clear();
    st.update_quota_alert(&q(85.0, 10.0, cycle_end.add_secs(30.0 * 86_400.0)), now);
    assert!(!st.active_quota_notice(now).is_empty(), "a new billing cycle should alert again");
    assert!(st.active_quota_notice(now.add_secs(20.0)).is_empty(), "the notice should fade");
    let _ = std::fs::remove_file(path);
}

#[test]
fn focus() {
    let now = Stamp::now();
    let mut st = fresh();
    let sounds = Rc::new(Cell::new(0));
    let counter = sounds.clone();
    st.sound = Box::new(move |_| counter.set(counter.get() + 1));
    st.foreground = foreground("Cursor", "login.cs - demo - Cursor", "42");
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "quiet", "generation_id", "quiet-g", "prompt", "安静", "workspace_roots", r"D:\work\demo"]), now);
    assert!(st.hint(st.live(), now).contains("点标题跳回对话"), "hint should offer the jump");
    st.apply(&ev(&["hook_event_name", "stop", "conversation_id", "quiet", "generation_id", "quiet-g", "status", "completed"]), now);
    st.play_pending_sounds(now);
    assert!(!st.live().sound_pending && st.live().sound_played, "a quiet finish should still settle the sound flag");
    assert_eq!(sounds.get(), 0, "a focused finish must not make a sound");
    assert!(st.live().done_until.unwrap().secs_since(now) <= 5.0, "a focused finish should leave quickly");
    st.foreground = foreground("Cursor", "other.cs - another-project - Cursor", "43");
    assert!(!st.session_focused(st.live()), "another Cursor window is not this session");

    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "cli", "generation_id", "cli-g", "prompt", "终端", "host_window", "134608", "host_process", "WindowsTerminal"]), now);
    let s = st.sessions["cli"].clone();
    assert!(s.host_window == 134608 && s.host_process == "WindowsTerminal", "host window was not recorded");
    assert!(BuddyState::can_jump(Some(&s)), "a terminal session without workspace roots can still jump");
    st.foreground = foreground("WindowsTerminal", "Some chat title", "134608");
    assert!(st.session_focused(&s), "the terminal running the agent counts as focused");
    st.foreground = foreground("WindowsTerminal", "Some chat title", "999");
    assert!(!st.session_focused(&s), "another terminal window is not this session");

    let mut st = fresh();
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "loud", "generation_id", "loud-g", "prompt", "提醒"]), now);
    st.apply(&ev(&["hook_event_name", "stop", "conversation_id", "loud", "generation_id", "loud-g", "status", "error"]), now);
    let played = Rc::new(RefCell::new(String::new()));
    let sink = played.clone();
    st.sound = Box::new(move |kind| *sink.borrow_mut() = kind.to_string());
    st.play_pending_sounds(now);
    assert_eq!(*played.borrow(), "error", "an unwatched failure should sound");
}

#[test]
fn parsing() {
    let now = Stamp::now();
    let parsed = HookEvent::parse(r#"{"hook_event_name":"preToolUse","workspace_roots":["D:\\a\\proj","E:\\b"],"tool_input":{"file_path":"D:\\a\\proj\\src\\main.cs"},"tool_name":"Read"}"#);
    assert_eq!(parsed.workspace_roots().len(), 2, "workspace roots");
    let mut st = fresh();
    st.apply(&parsed, now);
    assert_eq!(st.live().project, "proj\u{3001}b", "project name");
    assert_eq!(st.live().detail, "main.cs", "tool detail from tool_input");

    let truncated = HookEvent::parse(r#"{"hook_event_name":"stop","conversation_id":"cut","status":"error","text":"unterminated"#);
    assert!(truncated.get("hook_event_name") == "stop" && truncated.get("status") == "error", "fallback for a cut-off body");

    assert_eq!(elapsed(Some(now.add_mins(-59.0).add_secs(-40.0)), now), "59:40", "elapsed minutes must not round up");
    assert!(elapsed(Some(now.add_mins(-100.0)), now).starts_with("1:40:"), "elapsed hours must not round up");
}

#[test]
fn english() {
    let now = Stamp::now();
    let mut st = fresh();
    lang::set_english(true);
    st.apply(&ev(&["hook_event_name", "beforeSubmitPrompt", "conversation_id", "en", "generation_id", "en-g", "prompt", "hi"]), now);
    assert!(st.live().headline == "Working" && st.live().activity == "Got your question", "English prompt state: {}", st.live().activity);
    assert_eq!(tool_phrase("Read", true), "Done reading", "English finished phrase");
    assert_eq!(tool_phrase("Shell", false), "Running a command", "English running phrase");
    st.apply(&ev(&["hook_event_name", "stop", "conversation_id", "en", "generation_id", "en-g", "status", "completed"]), now);
    let meta = st.meta_text(st.live(), now);
    assert!(st.live().headline == "All done" && meta.starts_with("Took ") && meta.ends_with(" steps"), "English finish: {meta}");
    lang::apply("en");
    assert!(lang::english(), "explicit English");
    lang::apply("zh");
    assert!(!lang::english(), "explicit Chinese");
}

#[test]
fn quota_parsing() {
    use crate::quota::{self, percent};
    lang::set_english(false);
    let now = Stamp::now();
    let usage = r#"{"billingCycleStart":"1793770164000","billingCycleEnd":"1796362164000","planUsage":{"totalSpend":18888,"includedSpend":7000,"bonusSpend":11888,"limit":7000,"autoPercentUsed":13.06,"apiPercentUsed":29.2,"totalPercentUsed":14.4},"spendLimitUsage":{"limitType":"user","individualLimit":2000,"individualUsed":550}}"#;
    let plan = r#"{"planInfo":{"planName":"Pro+","price":"$60/mo"}}"#;
    let s = quota::parse(usage, plan, "pro_plus", now);
    assert!(s.ok, "parse failed: {}", s.error);
    assert_eq!(s.plan_name, "Pro+");
    assert!((s.api_percent.unwrap() - 29.2).abs() < 0.001, "api percent");
    assert!(s.total_spend_cents == Some(18888) && s.limit_cents == Some(7000), "spend fields");
    assert!(s.on_demand_limit_cents == Some(2000) && s.on_demand_used_cents == Some(550), "on-demand fields");
    // 2026-12-04 05:29:24 UTC
    assert_eq!(s.cycle_end, Some(Stamp(1_796_362_164_000)), "cycle end");
    assert_eq!(quota::money(18888), "$188.88", "money format");
    assert_eq!(percent(Some(0.4)), "<1%", "percent format");

    let round = QuotaSnapshot::deserialize(&s.serialize()).expect("cache roundtrip");
    assert_eq!(round.serialize(), s.serialize(), "cache roundtrip changed the snapshot");

    let fallback = quota::parse(r#"{"planUsage":{"totalSpend":3500,"limit":7000}}"#, "", "pro", now);
    assert_eq!(fallback.plan_name, "Pro", "membership fallback");
    assert!((fallback.headline_percent().unwrap() - 50.0).abs() < 0.001, "headline percent fallback");

    assert!(!quota::parse("[1,2]", "", "", now).ok, "non-object usage should fail");

    let mut plan_cache = None;
    let mut plan_at = None;
    let missing = quota::fetch_once(&std::env::temp_dir().join("no-such-cursor-state.vscdb"), &mut plan_cache, &mut plan_at);
    assert!(!missing.ok && !missing.error.is_empty(), "missing database should report an error");

    let rows = quota::rows(Some(&s), false, Stamp(1_796_362_164_000 - 9 * 86_400_000 - 3_600_000));
    assert_eq!(rows.len(), 1, "collapsed bar is one row");
    assert!(rows[0].text == "14%" && rows[0].right == "API 29% · 剩 10 天", "collapsed row: {:?}", rows[0]);
    assert!(quota::rows(Some(&s), true, now).len() >= 6, "expanded rows");
}

#[test]
fn quota_cache_from_csharp() {
    // Written by the C# host's QuotaSnapshot.Serialize; the Rust host reads the same file.
    let text = "ok=1\nerror=\nplan=Pro\nauto=13.06\napi=29.2\ntotal=-1\nspend=3500\nincluded=-1\nbonus=-1\nlimit=7000\nondemand=-1\nondemandLimit=-1\nstart=0\nend=639312564049999976\nfetched=639312564049999976\n";
    let s = QuotaSnapshot::deserialize(text).expect("parse C# cache");
    assert!(s.ok && s.total_percent.is_none() && s.limit_cents == Some(7000) && s.cycle_start.is_none());
    assert!((s.headline_percent().unwrap() - 50.0).abs() < 0.001);
    assert_eq!(s.fetched_at.unwrap().to_dotnet_ticks() / 10_000, 639312564049999976 / 10_000);
}

#[test]
fn updates() {
    use crate::update::{is_newer, UpdateInfo, REPOSITORY};
    assert!(is_newer("v1.2.0", "1.1.0"), "1.2.0 is newer");
    assert!(is_newer("v1.10", "1.9.3"), "1.10 is newer than 1.9.3");
    assert!(!is_newer("v1.1.0", "1.1.0"), "same version is not newer");
    assert!(!is_newer("v1.0.9-beta", "1.1.0"), "older pre-release is not newer");
    assert!(!is_newer("nightly", "1.1.0"), "unparseable tags are ignored");
    let parsed = UpdateInfo::parse(r#"{"tag_name":"v9.0.0","html_url":"https://github.com/x/y/releases/tag/v9.0.0"}"#, None);
    assert!(parsed.tag == "v9.0.0" && parsed.newer() && parsed.url.ends_with("/v9.0.0"), "release json");
    let hostile = UpdateInfo::parse(r#"{"tag_name":"v9.0.0","html_url":"file:///C:/evil.exe"}"#, None);
    assert!(hostile.url.starts_with(&format!("https://github.com/{REPOSITORY}")), "only GitHub release pages are opened");
    let path = temp_file("cursor-done-notify-update");
    let mut saved = parsed.clone();
    saved.notified_tag = "v9.0.0".into();
    saved.save(&path);
    let loaded = UpdateInfo::load(&path).expect("update cache round trip");
    assert!(loaded.tag == "v9.0.0" && loaded.notified_tag == "v9.0.0");
    assert!((loaded.checked.unwrap().0 - saved.checked.unwrap().0).abs() < 1000, "checked time survives");
    let _ = std::fs::remove_file(path);
}

#[test]
fn settings_file() {
    let path = temp_file("cursor-done-notify-settings");
    let saved = Settings {
        mute: true,
        quiet: false,
        theme: "dark".into(),
        language: "en".into(),
        seconds: 20,
        updates: false,
        stale_minutes: 60,
        ..Settings::default()
    };
    saved.save(&path);
    let mut loaded = Settings::load(&path);
    assert_eq!(loaded, saved, "settings round trip");
    assert!(loaded.quota, "quota defaults on");
    let mut st = fresh();
    st.apply_timing(&loaded);
    assert!(st.done_seconds == 20 && st.stale_minutes == 60, "menu timing wins");
    loaded.seconds = 500;
    st.apply_timing(&loaded);
    assert_eq!(st.done_seconds, 60, "seconds are clamped");
    assert_eq!(Settings::load(&path.with_extension("missing")).language, "auto", "defaults without a file");
    let _ = std::fs::remove_file(path);
}
