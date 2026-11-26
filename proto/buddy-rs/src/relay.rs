// hook-relay: Cursor runs this for every hook. It answers at once, drops the event into
// the inbox for the buddy host, and starts the host if none is running.
mod jsonscan;

use std::io::{Read, Write};
use std::path::PathBuf;

const MAX_STORED: usize = 2 * 1024 * 1024;
const COMPACT_OVER: usize = 32 * 1024;

fn main() {
    let reply = std::env::args().nth(1).unwrap_or_else(|| "empty".into());
    let body = read_stdin();
    if !body.trim().is_empty() {
        let event = add_host_window(&compact(&body));
        if write_inbox(&event).is_ok() {
            ensure_host();
        }
    }
    let answer = match reply.as_str() {
        "allow" => r#"{"permission":"allow"}"#,
        "continue" => r#"{"continue":true}"#,
        _ => "{}",
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{answer}");
    let _ = out.flush();
}

fn read_stdin() -> String {
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut stdin = std::io::stdin().lock();
    // Keep reading past the cap so the writer never blocks on a full pipe.
    while let Ok(n) = stdin.read(&mut chunk) {
        if n == 0 {
            break;
        }
        let room = MAX_STORED.saturating_sub(kept.len());
        kept.extend_from_slice(&chunk[..n.min(room)]);
    }
    decode(&kept)
}

/// Windows PowerShell may pipe UTF-16; everything else arrives as UTF-8.
fn decode(bytes: &[u8]) -> String {
    let utf16 = (bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE)
        || bytes.iter().take(160).filter(|b| **b == 0).count() > 8;
    if utf16 {
        let start = if bytes.starts_with(&[0xFF, 0xFE]) { 2 } else { 0 };
        let units: Vec<u16> = bytes[start..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    let text = String::from_utf8_lossy(bytes);
    text.strip_prefix('\u{feff}').unwrap_or(&text).to_string()
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// Bodies over 32 KB keep only the fields the host reads, each cut to a sensible length.
fn compact(body: &str) -> String {
    if body.len() <= COMPACT_OVER {
        return body.to_string();
    }
    let Some(event) = jsonscan::string_field(body, "hook_event_name") else {
        let mut end = body.len().min(48_000);
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        return body[..end].to_string();
    };
    let fields: [(&str, usize); 20] = [
        ("conversation_id", usize::MAX),
        ("generation_id", usize::MAX),
        ("status", usize::MAX),
        ("tool_name", usize::MAX),
        ("tool_use_id", usize::MAX),
        ("subagent_id", usize::MAX),
        ("tool_call_id", usize::MAX),
        ("error_message", 180),
        ("command", 180),
        ("file_path", 180),
        ("path", 180),
        ("target_file", 180),
        ("pattern", 120),
        ("query", 120),
        ("url", 180),
        ("glob", 120),
        ("prompt", 80),
        ("task", 120),
        ("agent_message", 180),
        ("text", 180),
    ];
    let mut map = serde_json::Map::new();
    map.insert("hook_event_name".into(), event.into());
    for (name, max) in fields {
        if let Some(value) = jsonscan::string_field(body, name).filter(|v| !v.is_empty()) {
            map.insert(name.into(), truncate(&value, max).into());
        }
    }
    if let Some(roots) = jsonscan::array_field(body, "workspace_roots").and_then(|r| serde_json::from_str(r).ok()) {
        map.insert("workspace_roots".into(), roots);
    }
    serde_json::Value::Object(map).to_string()
}

/// The first event of a turn records which top-level window runs the agent (Cursor or a
/// terminal), so the buddy can tell whether the user is looking at it and jump back to it.
fn add_host_window(body: &str) -> String {
    if jsonscan::string_field(body, "hook_event_name").as_deref() != Some("beforeSubmitPrompt") {
        return body.to_string();
    }
    let Some(brace) = body.find('{') else { return body.to_string() };
    let Some((window, process)) = platform::host_window() else { return body.to_string() };
    insert_host_fields(body, brace, window, &process)
}

fn insert_host_fields(body: &str, brace: usize, window: i64, process: &str) -> String {
    let rest = &body[brace + 1..];
    let mut fields = format!(
        "\"host_window\":\"{window}\",\"host_process\":{}",
        serde_json::Value::String(process.to_string())
    );
    if !rest.trim_start().starts_with('}') && !rest.trim().is_empty() {
        fields.push(',');
    }
    format!("{}{fields}{rest}", &body[..=brace])
}

fn state_dir() -> PathBuf {
    std::env::temp_dir().join("cursor-done-notify")
}

fn write_inbox(body: &str) -> std::io::Result<()> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let dir = state_dir().join("inbox");
    std::fs::create_dir_all(&dir)?;
    // Zero-padded .NET ticks first, so sorting names gives arrival order for either host.
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let ticks = nanos / 100 + 621_355_968_000_000_000;
    let random = |salt: u64| {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(salt);
        h.finish()
    };
    let name = format!("{ticks:020}-{:016x}{:016x}.json", random(std::process::id() as u64), random(nanos as u64));
    let final_path = dir.join(name);
    let temp_path = final_path.with_extension("json.tmp");
    std::fs::write(&temp_path, body.as_bytes())?;
    std::fs::rename(&temp_path, &final_path)
}

fn ensure_host() {
    let Some(host) = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(platform::HOST_NAME))) else { return };
    if host.exists() {
        platform::ensure_host(&host, &state_dir());
    }
}

#[cfg(windows)]
mod platform {
    use std::collections::HashMap;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, FILETIME, HWND, INVALID_HANDLE_VALUE, LPARAM, WAIT_ABANDONED, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows_sys::Win32::System::Threading::{
        CreateMutexW, CreateProcessW, GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, ReleaseMutex,
        WaitForSingleObject, CREATE_NO_WINDOW, DETACHED_PROCESS, PROCESS_INFORMATION, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        STARTUPINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindow, GetWindowThreadProcessId, IsWindowVisible, GW_OWNER};

    pub const HOST_NAME: &str = "buddy-host.exe";
    const DOTNET_TICKS_AT_1601: u64 = 504_911_232_000_000_000;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Search {
        pid: u32,
        found: HWND,
    }

    // Same rule as .NET's Process.MainWindowHandle: the first visible, unowned top-level window.
    unsafe extern "system" fn main_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam as *mut Search);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == search.pid && IsWindowVisible(hwnd) != 0 && GetWindow(hwnd, GW_OWNER).is_null() {
            search.found = hwnd;
            return 0;
        }
        1
    }

    /// Walks up the parent processes to the first one with a main window, stopping at Explorer.
    pub fn host_window() -> Option<(i64, String)> {
        let mut parents: HashMap<u32, (u32, String)> = HashMap::new();
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
                    let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
                    let stem = exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(&exe).to_string();
                    parents.insert(entry.th32ProcessID, (entry.th32ParentProcessID, stem));
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
        }
        let mut pid = std::process::id();
        for _ in 0..12 {
            let parent = parents.get(&pid)?.0;
            if parent <= 4 || parent == pid {
                return None;
            }
            pid = parent;
            let name = parents.get(&pid).map(|p| p.1.clone()).unwrap_or_default();
            if name.eq_ignore_ascii_case("explorer") {
                return None;
            }
            let mut search = Search { pid, found: null_mut() };
            unsafe { EnumWindows(Some(main_window), &mut search as *mut Search as LPARAM) };
            if !search.found.is_null() {
                return Some((search.found as isize as i64, name));
            }
        }
        None
    }

    fn host_running(pid_file: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(pid_file) else { return false };
        let mut parts = text.trim().split('|');
        let (Some(Ok(pid)), Some(Ok(expected))) = (parts.next().map(str::parse::<u32>), parts.next().map(str::parse::<u64>)) else {
            return false;
        };
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let alive = GetExitCodeProcess(handle, &mut code) != 0 && code == 259;
            let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            let (mut created, mut a, mut b, mut c) = (zero, zero, zero, zero);
            let timed = GetProcessTimes(handle, &mut created, &mut a, &mut b, &mut c) != 0;
            let ticks = (((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64) + DOTNET_TICKS_AT_1601;
            let mut buf = vec![0u16; 1024];
            let mut size = buf.len() as u32;
            let named = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut size) != 0;
            CloseHandle(handle);
            let path = String::from_utf16_lossy(&buf[..size as usize]);
            let stem = Path::new(&path).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            // .NET rounds the start time through local time; allow for the last few ticks.
            alive && timed && named && stem == "buddy-host" && ticks.abs_diff(expected) < 10_000
        }
    }

    pub fn ensure_host(host: &Path, state_dir: &Path) {
        let pid_file = state_dir.join("host.pid");
        unsafe {
            let name = wide(r"Local\CursorDoneNotifyLaunch");
            let mutex = CreateMutexW(null(), 0, name.as_ptr());
            if mutex.is_null() {
                return;
            }
            let wait = WaitForSingleObject(mutex, 2000);
            let owned = wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED;
            if owned && !host_running(&pid_file) {
                start(host);
                for _ in 0..20 {
                    if host_running(&pid_file) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            }
            if owned {
                ReleaseMutex(mutex);
            }
            CloseHandle(mutex);
        }
    }

    // Started without inheriting any handles: the host must not hold Cursor's hook pipes open.
    unsafe fn start(host: &Path) {
        let app: Vec<u16> = host.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut cmd: Vec<u16> = format!("\"{}\"", host.display()).encode_utf16().chain(std::iter::once(0)).collect();
        let dir: Vec<u16> = host.parent().unwrap_or(host).as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessW(
            app.as_ptr(),
            cmd.as_mut_ptr(),
            null(),
            null(),
            0,
            CREATE_NO_WINDOW | DETACHED_PROCESS,
            null(),
            dir.as_ptr(),
            &si,
            &mut pi,
        );
        if ok != 0 {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::path::Path;
    use std::process::{Command, Stdio};

    pub const HOST_NAME: &str = "buddy-host";

    pub fn host_window() -> Option<(i64, String)> {
        None
    }

    /// The host holds an exclusive lock on host.lock while it runs.
    pub fn ensure_host(host: &Path, state_dir: &Path) {
        use std::os::unix::io::AsRawFd;
        let _ = std::fs::create_dir_all(state_dir);
        let Ok(lock) = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(state_dir.join("host.lock")) else { return };
        let free = unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if !free {
            return;
        }
        drop(lock);
        let _ = Command::new(host).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16_and_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "{\"a\":\"你好\"}".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode(&bytes), "{\"a\":\"你好\"}");
        assert_eq!(decode("\u{feff}{}".as_bytes()), "{}");
    }

    #[test]
    fn small_bodies_pass_through() {
        let body = r#"{"hook_event_name":"stop","text":"hi"}"#;
        assert_eq!(compact(body), body);
    }

    #[test]
    fn large_bodies_keep_what_the_host_reads() {
        let big = "x".repeat(40_000);
        let body = format!(
            r#"{{"hook_event_name":"postToolUse","conversation_id":"c1","tool_name":"Read","tool_input":{{"file_path":"D:\\a\\b.cs","content":"{big}"}},"workspace_roots":["D:\\a"],"text":"{big}"}}"#
        );
        let out = compact(&body);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("compact output is JSON");
        assert_eq!(parsed["hook_event_name"], "postToolUse");
        assert_eq!(parsed["conversation_id"], "c1");
        assert_eq!(parsed["file_path"], r"D:\a\b.cs");
        assert_eq!(parsed["workspace_roots"][0], r"D:\a");
        assert_eq!(parsed["text"].as_str().unwrap().len(), 180);
        assert!(out.len() < 2_000);
    }

    #[test]
    fn host_fields_go_first() {
        let out = insert_host_fields(r#"{"hook_event_name":"beforeSubmitPrompt"}"#, 0, 42, "Cursor");
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["host_window"], "42");
        assert_eq!(parsed["host_process"], "Cursor");
        assert_eq!(parsed["hook_event_name"], "beforeSubmitPrompt");
        let empty = insert_host_fields("{}", 0, 7, "pwsh");
        assert!(serde_json::from_str::<serde_json::Value>(&empty).is_ok(), "{empty}");
    }
}
