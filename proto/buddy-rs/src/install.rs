// `buddy-host --install` / `--uninstall`: registers hook-relay in the user-level Cursor
// hooks.json. Same rules as install.ps1, which Windows keeps using: other people's hooks
// and unknown fields survive, a backup is kept, and a broken file is never overwritten.
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

struct Spec {
    name: &'static str,
    reply: &'static str,
}

const SAFE: [Spec; 7] = [
    Spec { name: "beforeSubmitPrompt", reply: "continue" },
    Spec { name: "postToolUse", reply: "empty" },
    Spec { name: "postToolUseFailure", reply: "empty" },
    Spec { name: "afterAgentThought", reply: "empty" },
    Spec { name: "afterAgentResponse", reply: "empty" },
    Spec { name: "subagentStop", reply: "empty" },
    Spec { name: "stop", reply: "empty" },
];

// These answer "allow", so they take part in Cursor's permission decisions; opt-in only.
const REALTIME: [Spec; 2] = [Spec { name: "preToolUse", reply: "allow" }, Spec { name: "subagentStart", reply: "allow" }];

/// The command line Cursor runs: PowerShell on Windows, a POSIX shell elsewhere.
pub fn hook_command(relay: &Path, reply: &str) -> String {
    let path = relay.to_string_lossy();
    if cfg!(windows) {
        format!("& '{}' {reply}", path.replace('\'', "''"))
    } else {
        format!("'{}' {reply}", path.replace('\'', r"'\''"))
    }
}

/// The relay path a hook command runs, when it has the `<relay> empty|allow|continue` shape.
fn relay_path(command: &str) -> Option<String> {
    let c = command.trim();
    let c = c.strip_prefix('&').map(str::trim_start).unwrap_or(c);
    let (rest, reply) = c.rsplit_once(char::is_whitespace)?;
    if !matches!(reply, "empty" | "allow" | "continue") {
        return None;
    }
    let p = rest.trim();
    let quoted = |q: char| p.len() >= 2 && p.starts_with(q) && p.ends_with(q);
    Some(if quoted('\'') {
        p[1..p.len() - 1].replace(r"'\''", "'").replace("''", "'")
    } else if quoted('"') {
        p[1..p.len() - 1].to_string()
    } else {
        p.to_string()
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let full = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let text = full.to_string_lossy().into_owned();
        // Both the default macOS and Windows file systems ignore case.
        if cfg!(any(windows, target_os = "macos")) { text.to_lowercase() } else { text }
    };
    norm(a) == norm(b)
}

fn is_owned(command: &str, relay: &Path) -> bool {
    relay_path(command).is_some_and(|p| same_file(Path::new(&p), relay))
}

fn read_config(path: &Path) -> Result<Value, String> {
    let mut cfg = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("现有 hooks.json 不是有效 JSON，未作修改：{}\n{e}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(format!("读不了 {}：{e}", path.display())),
    };
    let Some(map) = cfg.as_object_mut() else {
        return Err(format!("hooks.json 顶层不是对象，未作修改：{}", path.display()));
    };
    map.entry("version").or_insert(json!(1));
    if !map.get("hooks").is_some_and(Value::is_object) {
        map.insert("hooks".into(), Value::Object(Map::new()));
    }
    Ok(cfg)
}

fn write_config(path: &Path, cfg: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        let backup = path.with_extension("json.bak");
        std::fs::copy(path, &backup).map_err(|e| format!("备份 hooks.json 失败：{e}"))?;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temp, text + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        e.to_string()
    })
}

/// Removes this relay's entries from every hook list; returns how many went.
fn strip(cfg: &mut Value, relay: &Path) -> usize {
    let Some(hooks) = cfg.get_mut("hooks").and_then(Value::as_object_mut) else { return 0 };
    let mut removed = 0;
    let names: Vec<String> = hooks.keys().cloned().collect();
    for name in names {
        let Some(Value::Array(list)) = hooks.get_mut(&name) else { continue };
        let before = list.len();
        list.retain(|item| !item.get("command").and_then(Value::as_str).is_some_and(|c| is_owned(c, relay)));
        removed += before - list.len();
        if list.is_empty() && before > 0 {
            hooks.remove(&name);
        }
    }
    removed
}

fn merge(cfg: &mut Value, relay: &Path, realtime: bool) {
    strip(cfg, relay);
    let hooks = cfg["hooks"].as_object_mut().expect("read_config made hooks an object");
    let specs = SAFE.iter().chain(if realtime { REALTIME.iter() } else { [].iter() });
    for spec in specs {
        let entry = json!({ "command": hook_command(relay, spec.reply), "timeout": 5 });
        match hooks.get_mut(spec.name) {
            Some(Value::Array(list)) => list.push(entry),
            _ => {
                hooks.insert(spec.name.into(), json!([entry]));
            }
        }
    }
}

pub fn install_into(hooks: &Path, relay: &Path, realtime: bool) -> Result<(), String> {
    let mut cfg = read_config(hooks)?;
    merge(&mut cfg, relay, realtime);
    write_config(hooks, &cfg)
}

pub fn uninstall_from(hooks: &Path, relay: &Path) -> Result<usize, String> {
    if !hooks.exists() {
        return Ok(0);
    }
    let mut cfg = read_config(hooks)?;
    let removed = strip(&mut cfg, relay);
    if removed > 0 {
        write_config(hooks, &cfg)?;
    }
    Ok(removed)
}

fn user_hooks() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    Some(PathBuf::from(home).join(".cursor").join("hooks.json"))
}

fn relay_next_to_me() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = if cfg!(windows) { "hook-relay.exe" } else { "hook-relay" };
    let relay = exe.parent().ok_or("找不到程序所在目录")?.join(name);
    if !relay.exists() {
        return Err(format!("找不到中继：{}", relay.display()));
    }
    Ok(std::fs::canonicalize(&relay).unwrap_or(relay))
}

/// Stops a running host so the next hook starts the freshly installed one.
fn stop_host() {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let dir = crate::state_dir();
        let Ok(lock) = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(dir.join("host.lock")) else { return };
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return;
        }
        let pid = std::fs::read_to_string(dir.join("host.pid")).ok().and_then(|t| t.trim().parse::<i32>().ok());
        if let Some(pid) = pid.filter(|p| *p > 1) {
            unsafe { libc::kill(pid, libc::SIGTERM) };
            for _ in 0..40 {
                if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

fn clear_state() {
    let dir = crate::state_dir();
    let _ = std::fs::remove_dir_all(dir.join("inbox"));
    for name in ["live-view.txt", "last-error.txt", "host.pid", "quota-cache.txt", "quota-alert.txt"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

pub fn run_install(realtime: bool) -> Result<String, String> {
    let relay = relay_next_to_me()?;
    let hooks = user_hooks().ok_or("找不到用户目录")?;
    stop_host();
    install_into(&hooks, &relay, realtime)?;
    let mut out = format!("安装完成：{}", hooks.display());
    if !realtime {
        out += "\n已使用安全模式：不注册会返回 allow 的权限 Hook。需要实时工具动作时加 --realtime 重新安装。";
    }
    out += "\n请完全退出 Cursor 后重新打开。开始一轮 Agent 对话后，点点会出现在屏幕右下角。";
    Ok(out)
}

pub fn run_uninstall() -> Result<String, String> {
    let relay = relay_next_to_me()?;
    let hooks = user_hooks().ok_or("找不到用户目录")?;
    stop_host();
    let removed = uninstall_from(&hooks, &relay)?;
    clear_state();
    let first = if removed > 0 { format!("已从 hooks 中移除 {removed} 条点点记录：{}", hooks.display()) } else { "hooks 里没有点点的记录。".into() };
    Ok(first + "\n点点已关闭。请完全退出 Cursor 后重新打开。")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported from tests/hooks.ps1.
    #[test]
    fn install_keeps_other_hooks_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("cursor-done-notify-install-{}-{:?}", std::process::id(), std::thread::current().id()));
        let root = dir.join("install folder with spaces");
        std::fs::create_dir_all(&root).unwrap();
        let relay = root.join(if cfg!(windows) { "hook-relay.exe" } else { "hook-relay" });
        std::fs::write(&relay, b"").unwrap();
        let hooks = dir.join(".cursor").join("hooks.json");
        std::fs::create_dir_all(hooks.parent().unwrap()).unwrap();

        let foreign = if cfg!(windows) { r"C:\keep\hook-relay.exe empty" } else { "/keep/hook-relay empty" };
        let other = "/keep/other-hook.sh";
        let old_spelling = format!("\"{}\" empty", relay.display());
        let start = json!({
            "version": 1,
            "futureTopLevel": "keep-me",
            "hooks": {
                "stop": [
                    { "command": foreign, "timeout": 10, "futureField": "keep-me" },
                    { "command": format!("{} empty", relay.display()), "timeout": 5 },
                    { "command": old_spelling, "timeout": 5 }
                ],
                "preToolUse": [{ "command": hook_command(&relay, "allow"), "timeout": 5 }],
                "postToolUse": [{ "command": other, "timeout": 9, "matcher": "Read" }],
                "beforeSubmitPrompt": [{ "type": "prompt", "prompt": "Keep this policy", "timeout": 7 }]
            }
        });
        std::fs::write(&hooks, serde_json::to_string(&start).unwrap()).unwrap();

        let read = || serde_json::from_str::<Value>(&std::fs::read_to_string(&hooks).unwrap()).unwrap();
        let commands = |cfg: &Value, name: &str| -> Vec<String> {
            cfg["hooks"][name].as_array().map_or(Vec::new(), |l| l.iter().filter_map(|i| i["command"].as_str().map(String::from)).collect())
        };

        install_into(&hooks, &relay, false).unwrap();
        let cfg = read();
        let stop = commands(&cfg, "stop");
        assert_eq!(stop.iter().filter(|c| *c == foreign).count(), 1, "install dropped a same-named foreign hook: {stop:?}");
        assert_eq!(stop.iter().filter(|c| **c == hook_command(&relay, "empty")).count(), 1, "quoted own hook: {stop:?}");
        assert_eq!(stop.iter().filter(|c| is_owned(c, &relay)).count(), 1, "another spelling of its own hook survived: {stop:?}");
        assert!(commands(&cfg, "preToolUse").iter().all(|c| !is_owned(c, &relay)), "safe install kept a permission hook");
        assert_eq!(cfg["futureTopLevel"], "keep-me", "unknown top-level field");
        assert_eq!(cfg["hooks"]["stop"][0]["futureField"], "keep-me", "unknown hook field");
        assert_eq!(cfg["hooks"]["postToolUse"][0]["matcher"], "Read", "another hook's matcher");
        assert_eq!(cfg["hooks"]["beforeSubmitPrompt"][0]["prompt"], "Keep this policy", "prompt hook without a command");
        assert!(hooks.with_extension("json.bak").exists(), "backup");

        install_into(&hooks, &relay, true).unwrap();
        let pre = commands(&read(), "preToolUse");
        assert_eq!(pre, vec![hook_command(&relay, "allow")], "opt-in permission hook");

        assert_eq!(uninstall_from(&hooks, &relay).unwrap(), 9, "all nine own hooks removed");
        let cfg = read();
        assert_eq!(commands(&cfg, "stop"), vec![foreign.to_string()], "uninstall kept the other hook");
        assert!(cfg["hooks"].get("preToolUse").is_none(), "empty lists are dropped");
        assert_eq!(cfg["hooks"]["beforeSubmitPrompt"][0]["prompt"], "Keep this policy");

        std::fs::write(&hooks, "{broken").unwrap();
        assert!(install_into(&hooks, &relay, false).is_err(), "install accepted invalid JSON");
        assert_eq!(std::fs::read_to_string(&hooks).unwrap(), "{broken", "install overwrote invalid JSON");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_every_command_spelling() {
        assert_eq!(relay_path("& 'C:\\a b\\hook-relay.exe' empty").as_deref(), Some("C:\\a b\\hook-relay.exe"));
        assert_eq!(relay_path("'/a/it'\\''s/hook-relay' allow").as_deref(), Some("/a/it's/hook-relay"));
        assert_eq!(relay_path("\"/a b/hook-relay\" continue").as_deref(), Some("/a b/hook-relay"));
        assert_eq!(relay_path("/a/hook-relay empty").as_deref(), Some("/a/hook-relay"));
        assert_eq!(relay_path("/a/other-hook.sh"), None);
        assert_eq!(relay_path("/a/hook-relay deny"), None);
    }
}
