use crate::buddy::{Buddy, Click};
use crate::menu::{self, Action, Entry};
use crate::lang::t;
use crate::state::{leaf, BuddyState, Foreground, Session, Settings};
use crate::Options;
use std::cell::RefCell;
use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::time::Instant;
use windows_sys::Win32::Foundation::{
    CloseHandle, BOOL, ERROR_SUCCESS, FILETIME, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WAIT_ABANDONED,
    WAIT_OBJECT_0, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MonitorFromPoint, ReleaseDC,
    SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
    WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use std::sync::atomic::{AtomicU32, Ordering};

const WM_TRAY: u32 = WM_APP + 1;
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

const MUTEX_NAME: &str = r"Local\CursorDoneNotifyHost";
// .NET DateTime ticks start at 0001-01-01, FILETIME at 1601-01-01; the relay compares .NET ticks.
const DOTNET_TICKS_AT_1601: u64 = 504_911_232_000_000_000;

struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
}

struct Drag {
    grab: POINT,
    start: POINT,
    moved: bool,
}

struct App {
    buddy: Buddy,
    surface: Option<Surface>,
    pos: POINT,
    drag: Option<Drag>,
    visible: bool,
    stay: bool,
    quit_at: Option<Instant>,
    position_path: PathBuf,
    tray_icon: HICON,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn run(opts: Options) {
    let state_dir = crate::state_dir();
    let _ = std::fs::create_dir_all(state_dir.join("inbox"));
    unsafe {
        let name = wide(MUTEX_NAME);
        let mutex = CreateMutexW(null(), 0, name.as_ptr());
        if mutex.is_null() {
            return;
        }
        let wait = WaitForSingleObject(mutex, 0);
        if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
            CloseHandle(mutex);
            return;
        }
        let pid_file = state_dir.join("host.pid");
        let pid_text = pid_line();
        let _ = std::fs::write(&pid_file, &pid_text);

        run_window(opts, &state_dir);

        if std::fs::read_to_string(&pid_file).is_ok_and(|t| t.trim() == pid_text) {
            let _ = std::fs::remove_file(&pid_file);
        }
        let _ = std::fs::remove_file(state_dir.join("live-view.txt"));
        CloseHandle(mutex);
    }
}

/// "pid|start ticks", the format hook-relay.exe checks before starting another host.
unsafe fn pid_line() -> String {
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user);
    let filetime = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
    format!("{}|{}", GetCurrentProcessId(), filetime + DOTNET_TICKS_AT_1601)
}

unsafe fn run_window(opts: Options, state_dir: &std::path::Path) {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let mut buddy = match Buddy::new(1.0, state_dir, Settings::default_path()) {
        Ok(b) => b,
        Err(e) => {
            let _ = std::fs::write(state_dir.join("last-error.txt"), &e);
            return;
        }
    };
    buddy.clear_stale();
    buddy.state.foreground = Box::new(foreground);
    buddy.state.sound = Box::new(play_sound);
    buddy.dark = wants_dark(&buddy.state.settings.theme);

    let instance = GetModuleHandleW(null());
    let class = wide("CursorDoneBuddyRs");
    let wc = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: class.as_ptr(),
    };
    RegisterClassW(&wc);
    let title = wide("点点");
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        class.as_ptr(),
        title.as_ptr(),
        WS_POPUP,
        0,
        0,
        1,
        1,
        null_mut(),
        null_mut(),
        instance,
        null(),
    );
    if hwnd.is_null() {
        return;
    }
    buddy.set_scale(GetDpiForWindow(hwnd) as f32 / 96.0);
    buddy.start_quota();
    buddy.start_update_check();
    let position_path = state_dir.join("buddy-position.txt");
    let (w, h) = buddy.size();
    let pos = initial_position(&position_path, w as i32, h as i32);
    let tray_icon = make_tray_icon();

    APP.with(|app| {
        *app.borrow_mut() = Some(App {
            buddy,
            surface: None,
            pos,
            drag: None,
            visible: false,
            stay: opts.stay,
            quit_at: opts.exit_after.map(|d| Instant::now() + d),
            position_path,
            tray_icon,
        });
    });
    let taskbar = wide("TaskbarCreated");
    TASKBAR_CREATED.store(RegisterWindowMessageW(taskbar.as_ptr()), Ordering::Relaxed);
    tray(hwnd, NIM_ADD, None);
    SetTimer(hwnd, 1, 33, None);
    on_timer(hwnd);

    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    APP.with(|app| {
        if let Some(mut a) = app.borrow_mut().take() {
            if let Some(s) = a.surface.take() {
                release(s);
            }
            if !a.tray_icon.is_null() {
                DestroyIcon(a.tray_icon);
            }
        }
    });
}

// ---- tray ----

unsafe fn copy_wide(dst: &mut [u16], text: &str) {
    let room = dst.len() - 1;
    for (d, s) in dst.iter_mut().zip(text.encode_utf16().take(room).chain(std::iter::once(0))) {
        *d = s;
    }
}

unsafe fn tray(hwnd: HWND, action: u32, balloon: Option<(&str, &str)>) {
    let mut data: NOTIFYICONDATAW = std::mem::zeroed();
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = 1;
    if action != NIM_DELETE {
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = with_app(|app| app.tray_icon).unwrap_or(null_mut());
        copy_wide(&mut data.szTip, &t("点点", "Diandian"));
    }
    if let Some((title, text)) = balloon {
        data.uFlags |= NIF_INFO;
        data.dwInfoFlags = NIIF_INFO;
        data.Anonymous.uTimeout = 6000;
        copy_wide(&mut data.szInfoTitle, title);
        copy_wide(&mut data.szInfo, text);
    }
    Shell_NotifyIconW(action, &data);
}

/// The head of the idle pose, cropped the way the C# host does it.
unsafe fn make_tray_icon() -> HICON {
    let Some(dir) = crate::sprites::find_pose_dir() else { return null_mut() };
    let Ok(source) = tiny_skia::Pixmap::load_png(dir.join("idle-0.png")) else { return null_mut() };
    let side = (source.width() as f32 * 0.66) as u32;
    let x0 = (source.width() - side) / 2;
    let y0 = (source.height() as f32 * 0.02) as u32;
    let Some(rect) = tiny_skia::IntRect::from_xywh(x0 as i32, y0 as i32, side, side) else { return null_mut() };
    let Some(crop) = source.clone_rect(rect) else { return null_mut() };
    let size = GetSystemMetrics(SM_CXSMICON).max(16) as u32;
    let icon = crate::sprites::area_downscale(&crop, size, size);

    let screen = GetDC(null_mut());
    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = size as i32;
    info.bmiHeader.biHeight = -(size as i32);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    let mut bits: *mut c_void = null_mut();
    let color = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    ReleaseDC(null_mut(), screen);
    if color.is_null() || bits.is_null() {
        return null_mut();
    }
    // Icons take straight (not premultiplied) BGRA.
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
    for (d, s) in dst.chunks_exact_mut(4).zip(icon.data().chunks_exact(4)) {
        let a = s[3] as u32;
        let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a / 2) / a).min(255) as u8 };
        d[0] = un(s[2]);
        d[1] = un(s[1]);
        d[2] = un(s[0]);
        d[3] = s[3];
    }
    let mask = CreateBitmap(size as i32, size as i32, 1, 1, null());
    let ii = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
    let handle = CreateIconIndirect(&ii);
    DeleteObject(mask);
    DeleteObject(color);
    handle
}

fn open_url(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    let verb = wide("open");
    let target = wide(url);
    unsafe { ShellExecuteW(null_mut(), verb.as_ptr(), target.as_ptr(), null(), null(), SW_SHOWNORMAL) };
}

fn lparam_point(lparam: LPARAM) -> (i32, i32) {
    ((lparam & 0xffff) as i16 as i32, ((lparam >> 16) & 0xffff) as i16 as i32)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            on_timer(hwnd);
            0
        }
        WM_LBUTTONDOWN => {
            let mut cursor = POINT { x: 0, y: 0 };
            GetCursorPos(&mut cursor);
            with_app(|app| {
                app.drag = Some(Drag { grab: cursor, start: app.pos, moved: false });
            });
            SetCapture(hwnd);
            0
        }
        WM_MOUSEMOVE => {
            let mut cursor = POINT { x: 0, y: 0 };
            GetCursorPos(&mut cursor);
            let moved = with_app(|app| {
                let Some(drag) = app.drag.as_mut() else { return false };
                let (dx, dy) = (cursor.x - drag.grab.x, cursor.y - drag.grab.y);
                if !drag.moved && dx.abs() + dy.abs() > 4 {
                    drag.moved = true;
                }
                if drag.moved {
                    app.pos = POINT { x: drag.start.x + dx, y: drag.start.y + dy };
                }
                drag.moved
            });
            if moved == Some(true) {
                present(hwnd);
            }
            0
        }
        WM_LBUTTONUP => {
            ReleaseCapture();
            let (x, y) = lparam_point(lparam);
            let action = with_app(|app| {
                let drag = app.drag.take()?;
                if drag.moved {
                    let (w, h) = app.buddy.size();
                    app.pos = clamp_to_work_area(app.pos, w as i32, h as i32);
                    let _ = std::fs::write(&app.position_path, format!("{},{}", app.pos.x, app.pos.y));
                    return None;
                }
                Some(app.buddy.click(x as f32, y as f32))
            })
            .flatten();
            match action {
                Some(Click::Dismiss) => close(hwnd),
                Some(Click::Focus(session)) => show_session_window(&session),
                _ => {}
            }
            on_timer(hwnd);
            0
        }
        WM_SETCURSOR => {
            let mut cursor = POINT { x: 0, y: 0 };
            GetCursorPos(&mut cursor);
            let hand = with_app(|app| app.buddy.hit_hand((cursor.x - app.pos.x) as f32, (cursor.y - app.pos.y) as f32)).unwrap_or(false);
            SetCursor(LoadCursorW(null_mut(), if hand { IDC_HAND } else { IDC_SIZEALL }));
            1
        }
        WM_RBUTTONUP => {
            show_menu(hwnd);
            0
        }
        WM_DPICHANGED => {
            let dpi = (wparam & 0xffff) as f32;
            with_app(|app| app.buddy.set_scale(dpi / 96.0));
            on_timer(hwnd);
            0
        }
        WM_TRAY => {
            let event = (lparam & 0xffff) as u32;
            if event == WM_LBUTTONUP || event == WM_RBUTTONUP {
                show_menu(hwnd);
            }
            0
        }
        WM_DESTROY => {
            tray(hwnd, NIM_DELETE, None);
            PostQuitMessage(0);
            0
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            // Explorer restarted and forgot every tray icon.
            tray(hwnd, NIM_ADD, None);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.borrow_mut().as_mut().map(f))
}

unsafe fn close(hwnd: HWND) {
    let stay = with_app(|app| app.stay).unwrap_or(false);
    if stay {
        with_app(|app| {
            app.buddy.reset_after_close();
            app.visible = false;
        });
        ShowWindow(hwnd, SW_HIDE);
    } else {
        DestroyWindow(hwnd);
    }
}

unsafe fn on_timer(hwnd: HWND) {
    if with_app(|app| app.quit_at.is_some_and(|at| Instant::now() >= at)).unwrap_or(false) {
        DestroyWindow(hwnd);
        return;
    }
    let tick = with_app(|app| {
        // Hidden between turns (--stay): keep reading the inbox, draw nothing.
        let hidden = app.stay && !app.visible && !app.buddy.has_sessions();
        let tick = app.buddy.tick(Instant::now(), !hidden);
        (tick.close, !hidden && (tick.repaint || !app.visible))
    });
    let Some((should_close, repaint)) = tick else { return };
    if let Some(tag) = with_app(|app| app.buddy.take_update_balloon()).flatten() {
        let text = t(
            &format!("{tag} 已发布，右键点点点或托盘图标即可下载。"),
            &format!("{tag} is out. Right-click the buddy or the tray icon to download."),
        );
        tray(hwnd, NIM_MODIFY, Some((&t("点点有新版本", "Diandian update"), &text)));
    }
    if should_close {
        let hidden = with_app(|app| app.stay && !app.visible).unwrap_or(false);
        if hidden {
            with_app(|app| app.buddy.reset_after_close());
        } else {
            close(hwnd);
        }
        return;
    }
    if repaint {
        present(hwnd);
        let show = with_app(|app| !std::mem::replace(&mut app.visible, true)).unwrap_or(false);
        if show {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

unsafe fn present(hwnd: HWND) {
    with_app(|app| {
        let pixmap = app.buddy.canvas();
        let (w, h) = (pixmap.width(), pixmap.height());
        let screen = GetDC(null_mut());
        if let Some(old) = app.surface.as_ref().map(|s| (s.width, s.height)) {
            if old != (w, h) {
                // Keep the bottom edge where it was when the bubble grows or shrinks.
                app.pos.y += old.1 as i32 - h as i32;
                if let Some(s) = app.surface.take() {
                    release(s);
                }
            }
        }
        if app.surface.is_none() {
            app.surface = create(screen, w, h);
        }
        let Some(surface) = app.surface.as_ref() else {
            ReleaseDC(null_mut(), screen);
            return;
        };
        // tiny-skia is premultiplied RGBA; a 32-bit DIB is premultiplied BGRA.
        let src = pixmap.data();
        let dst = std::slice::from_raw_parts_mut(surface.bits, src.len());
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        let src_pt = POINT { x: 0, y: 0 };
        let size = SIZE { cx: w as i32, cy: h as i32 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        UpdateLayeredWindow(hwnd, screen, &app.pos, &size, surface.dc, &src_pt, 0, &blend, ULW_ALPHA);
        ReleaseDC(null_mut(), screen);
    });
}

unsafe fn create(screen: HDC, w: u32, h: u32) -> Option<Surface> {
    let dc = CreateCompatibleDC(screen);
    if dc.is_null() {
        return None;
    }
    let mut info: BITMAPINFO = std::mem::zeroed();
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w as i32,
        biHeight: -(h as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    let mut bits: *mut c_void = null_mut();
    let bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    if bitmap.is_null() || bits.is_null() {
        DeleteDC(dc);
        return None;
    }
    let old = SelectObject(dc, bitmap);
    Some(Surface { dc, bitmap, old, bits: bits as *mut u8, width: w, height: h })
}

unsafe fn release(s: Surface) {
    SelectObject(s.dc, s.old);
    DeleteObject(s.bitmap);
    DeleteDC(s.dc);
}

// ---- placement ----

unsafe fn work_area_at(pt: POINT) -> RECT {
    let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(monitor, &mut info) == 0 {
        return RECT { left: 0, top: 0, right: 1280, bottom: 720 };
    }
    info.rcWork
}

unsafe fn clamp_to_work_area(pos: POINT, w: i32, h: i32) -> POINT {
    let area = work_area_at(POINT { x: pos.x + w / 2, y: pos.y + h / 2 });
    POINT {
        x: pos.x.clamp(area.left, (area.right - w).max(area.left)),
        y: pos.y.clamp(area.top, (area.bottom - h).max(area.top)),
    }
}

unsafe fn initial_position(path: &std::path::Path, w: i32, h: i32) -> POINT {
    if let Ok(text) = std::fs::read_to_string(path) {
        let parts: Vec<i32> = text.trim().split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if parts.len() == 2 {
            return clamp_to_work_area(POINT { x: parts[0], y: parts[1] }, w, h);
        }
    }
    let mut cursor = POINT { x: 0, y: 0 };
    GetCursorPos(&mut cursor);
    let area = work_area_at(cursor);
    POINT { x: area.right - w - 16, y: area.bottom - h - 16 }
}

// ---- desktop integration ----

fn play_sound(kind: &str) {
    let style = match kind {
        "error" => MB_ICONHAND,
        "done" => MB_ICONASTERISK,
        _ => MB_ICONEXCLAMATION,
    };
    unsafe { MessageBeep(style) };
}

unsafe fn title_of(hwnd: HWND) -> String {
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

unsafe fn process_name_of(hwnd: HWND) -> String {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if handle.is_null() {
        return String::new();
    }
    let mut buf = vec![0u16; 1024];
    let mut size = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut size);
    CloseHandle(handle);
    if ok == 0 {
        return String::new();
    }
    let path = PathBuf::from(String::from_utf16_lossy(&buf[..size as usize]));
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn foreground() -> Option<Foreground> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return Some(Foreground { window: "0".into(), ..Foreground::default() });
        }
        Some(Foreground { process: process_name_of(hwnd), title: title_of(hwnd), window: (hwnd as isize).to_string() })
    }
}

unsafe fn bring_forward(hwnd: HWND) -> bool {
    if IsIconic(hwnd) != 0 {
        ShowWindow(hwnd, SW_RESTORE);
    }
    SetForegroundWindow(hwnd) != 0
}

struct Search {
    process: String,
    title_part: String,
    found: HWND,
}

unsafe extern "system" fn find_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam as *mut Search);
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    if !title_of(hwnd).to_lowercase().contains(&search.title_part) {
        return 1;
    }
    if !process_name_of(hwnd).eq_ignore_ascii_case(&search.process) {
        return 1;
    }
    search.found = hwnd;
    0
}

fn show_session_window(s: &Session) {
    unsafe {
        if BuddyState::terminal_host(s) {
            let hwnd = s.host_window as isize as HWND;
            if IsWindow(hwnd) != 0 && bring_forward(hwnd) {
                return;
            }
        }
        let name = leaf(&s.workspace_root);
        if name.is_empty() {
            return;
        }
        let mut search = Search { process: "Cursor".into(), title_part: name.to_lowercase(), found: null_mut() };
        EnumWindows(Some(find_window), &mut search as *mut Search as LPARAM);
        if !search.found.is_null() && bring_forward(search.found) {
            return;
        }
    }
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("cmd.exe")
        .args(["/c", "cursor", &s.workspace_root])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

fn wants_dark(theme: &str) -> bool {
    match theme {
        "dark" => true,
        "light" => false,
        _ => unsafe {
            let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
            let value = wide("AppsUseLightTheme");
            let mut data = 1u32;
            let mut size = 4u32;
            let status = RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                null_mut(),
                &mut data as *mut u32 as *mut c_void,
                &mut size,
            );
            status == ERROR_SUCCESS && data == 0
        },
    }
}

// ---- menu ----

unsafe fn build_menu(entries: &[Entry]) -> HMENU {
    let menu = CreatePopupMenu();
    for entry in entries {
        match entry {
            Entry::Item { id, label, checked, enabled, bold } => {
                let text = wide(label);
                let mut flags = MF_STRING;
                if *checked {
                    flags |= MF_CHECKED;
                }
                if !*enabled {
                    flags |= MF_GRAYED;
                }
                AppendMenuW(menu, flags, *id, text.as_ptr());
                if *bold {
                    SetMenuDefaultItem(menu, *id as u32, 0);
                }
            }
            Entry::Sub { label, items } => {
                let text = wide(label);
                AppendMenuW(menu, MF_POPUP, build_menu(items) as usize, text.as_ptr());
            }
            Entry::Separator => {
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
            }
        }
    }
    menu
}

unsafe fn show_menu(hwnd: HWND) {
    let Some(entries) = with_app(|app| menu::entries(&app.buddy)) else { return };
    let menu = build_menu(&entries);

    let mut cursor = POINT { x: 0, y: 0 };
    GetCursorPos(&mut cursor);
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, cursor.x, cursor.y, 0, hwnd, null()) as usize;
    DestroyMenu(menu);

    let action = with_app(|app| {
        let action = menu::apply(&mut app.buddy, cmd);
        if matches!(action, Action::Changed) {
            app.buddy.dark = wants_dark(&app.buddy.state.settings.theme);
        }
        action
    });
    match action {
        Some(Action::Quit) => {
            DestroyWindow(hwnd);
            return;
        }
        Some(Action::Open(url)) => open_url(&url),
        _ => {}
    }
    on_timer(hwnd);
}
