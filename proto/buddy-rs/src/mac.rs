// macOS window layer: a borderless, non-activating panel above other windows on every
// Space, showing the shared canvas as its layer contents, plus a menu bar item.
use crate::buddy::{Buddy, Click};
use crate::lang::t;
use crate::menu::{self, Action, Entry};
use crate::state::{Foreground, Session, Settings};
use crate::Options;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSControlStateValueOn, NSCursor, NSEvent, NSEventMask,
    NSEventType, NSImage, NSMenu, NSMenuItem, NSPanel, NSScreen, NSSound, NSStatusBar, NSStatusItem, NSStatusWindowLevel,
    NSVariableStatusItemLength, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage};
use objc2_foundation::{
    ns_string, NSObject, NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString, NSTimer, NSURL, NSUserDefaults,
};
use objc2_quartz_core::CALayer;
use std::cell::RefCell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and MenuTarget has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CursorDoneBuddyMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(pick:))]
        fn pick(&self, item: &NSMenuItem) {
            on_menu(item.tag() as usize);
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

struct Drag {
    grab: NSPoint,
    start: NSPoint,
    moved: bool,
}

struct App {
    buddy: Buddy,
    window: Retained<NSPanel>,
    layer: Retained<CALayer>,
    status: Retained<NSStatusItem>,
    target: Retained<MenuTarget>,
    drag: Option<Drag>,
    visible: bool,
    stay: bool,
    quit_at: Option<Instant>,
    position_path: PathBuf,
    shown_size: (u32, u32),
    ignoring: Option<bool>,
    menu_shown: String,
    menu_checked: Option<Instant>,
    // Held for the life of the process: hook-relay starts a host only when it can take this lock.
    _lock: std::fs::File,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.borrow_mut().as_mut().map(f))
}

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("AppKit runs on the main thread")
}

fn take_lock(state_dir: &Path) -> Option<std::fs::File> {
    use std::os::unix::io::AsRawFd;
    let file = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(state_dir.join("host.lock")).ok()?;
    (unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0).then_some(file)
}

pub fn run(opts: Options) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let state_dir = crate::state_dir();
    let _ = std::fs::create_dir_all(state_dir.join("inbox"));
    let Some(lock) = take_lock(&state_dir) else { return };

    let mut buddy = match Buddy::new(1.0, &state_dir, Settings::default_path()) {
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

    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon and no menu bar takeover, like LSUIElement.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let window = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(10.0, 10.0)),
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(false);
    window.setLevel(NSStatusWindowLevel);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    window.setHidesOnDeactivate(false);
    window.setFloatingPanel(true);
    window.setBecomesKeyOnlyIfNeeded(true);
    let Some(view) = window.contentView() else { return };
    view.setWantsLayer(true);
    let Some(layer) = view.layer() else { return };

    buddy.set_scale(window.backingScaleFactor() as f32);
    buddy.start_quota();
    buddy.start_update_check();

    let status = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = status.button(mtm) {
        if let Some(icon) = status_icon() {
            button.setImage(Some(&icon));
        }
        button.setToolTip(Some(&NSString::from_str(&t("点点", "Diandian"))));
    }

    let position_path = state_dir.join("buddy-position.txt");
    APP.with(|cell| {
        *cell.borrow_mut() = Some(App {
            buddy,
            window,
            layer,
            status,
            target: MenuTarget::new(mtm),
            drag: None,
            visible: false,
            stay: opts.stay,
            quit_at: opts.exit_after.map(|d| Instant::now() + d),
            position_path,
            shown_size: (0, 0),
            ignoring: None,
            menu_shown: String::new(),
            menu_checked: None,
            _lock: lock,
        });
    });

    let mask = NSEventMask::LeftMouseDown | NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp;
    let handler = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        let ev = unsafe { event.as_ref() };
        if handle_event(ev) { std::ptr::null_mut() } else { event.as_ptr() }
    });
    let _monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) };

    let tick = RcBlock::new(|_timer: NonNull<NSTimer>| on_tick());
    let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.033, true, &tick) };
    // Keep animating while a menu is open or the panel is being dragged.
    unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };

    on_tick();
    app.run();
}

fn quit() {
    let _ = std::fs::remove_file(crate::state_dir().join("live-view.txt"));
    NSApplication::sharedApplication(mtm()).terminate(None);
}

fn close() {
    let stay = with_app(|app| app.stay).unwrap_or(false);
    if !stay {
        quit();
        return;
    }
    with_app(|app| {
        app.buddy.reset_after_close();
        app.visible = false;
        app.window.orderOut(None);
    });
}

fn on_tick() {
    if with_app(|app| app.quit_at.is_some_and(|at| Instant::now() >= at)).unwrap_or(false) {
        quit();
        return;
    }
    let result = with_app(|app| {
        // Hidden between turns (--stay): keep reading the inbox, draw nothing.
        let hidden = app.stay && !app.visible && !app.buddy.has_sessions();
        app.buddy.set_scale(app.window.backingScaleFactor() as f32);
        let tick = app.buddy.tick(Instant::now(), !hidden);
        // Notifications need an app bundle; the menu item announces new releases instead.
        let _ = app.buddy.take_update_balloon();
        (tick.close, !hidden && (tick.repaint || !app.visible))
    });
    let Some((should_close, repaint)) = result else { return };
    if should_close {
        let hidden = with_app(|app| app.stay && !app.visible).unwrap_or(false);
        if hidden {
            with_app(|app| app.buddy.reset_after_close());
        } else {
            close();
        }
        return;
    }
    with_app(|app| {
        if repaint {
            present(app);
            if !app.visible {
                app.window.orderFrontRegardless();
                app.visible = true;
            }
        }
        update_click_through(app);
        refresh_status_menu(app);
    });
}

// ---- drawing ----

unsafe extern "C-unwind" fn free_pixels(info: *mut c_void, _data: NonNull<c_void>, _size: usize) {
    drop(Box::from_raw(info as *mut Vec<u8>));
}

/// Premultiplied RGBA bytes, as tiny-skia stores them, wrapped in a CGImage that owns a copy.
fn cg_image(data: &[u8], width: u32, height: u32) -> Option<CFRetained<CGImage>> {
    let bytes = Box::new(data.to_vec());
    let (ptr, len) = (bytes.as_ptr() as *const c_void, bytes.len());
    let info = Box::into_raw(bytes) as *mut c_void;
    let provider = unsafe { CGDataProvider::with_data(info, ptr, len, Some(free_pixels)) }?;
    let space = CGColorSpace::new_device_rgb()?;
    // kCGImageAlphaPremultipliedLast with the default (big-endian) byte order is RGBA.
    unsafe {
        CGImage::new(
            width as usize,
            height as usize,
            8,
            32,
            width as usize * 4,
            Some(&space),
            CGBitmapInfo(1),
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
}

fn present(app: &mut App) {
    let (w, h) = app.buddy.size();
    let Some(image) = cg_image(app.buddy.canvas().data(), w, h) else { return };
    let scale = app.window.backingScaleFactor();
    if app.shown_size != (w, h) {
        let size = NSSize::new(w as f64 / scale, h as f64 / scale);
        let origin = if app.shown_size == (0, 0) {
            initial_origin(&app.position_path, size)
        } else {
            // The origin is the bottom-left corner, so keeping it keeps the bottom edge in place.
            app.window.frame().origin
        };
        app.window.setFrame_display(NSRect::new(origin, size), true);
        app.shown_size = (w, h);
    }
    app.layer.setContentsScale(scale);
    let contents: &AnyObject = unsafe { &*(&*image as *const CGImage as *const AnyObject) };
    unsafe { app.layer.setContents(Some(contents)) };
}

/// Transparent pixels must not swallow clicks meant for the windows underneath.
fn update_click_through(app: &mut App) {
    if app.drag.is_some() || !app.visible {
        return;
    }
    let mouse = NSEvent::mouseLocation();
    let frame = app.window.frame();
    let scale = app.window.backingScaleFactor();
    let inside = mouse.x >= frame.origin.x
        && mouse.x < frame.origin.x + frame.size.width
        && mouse.y >= frame.origin.y
        && mouse.y < frame.origin.y + frame.size.height;
    let (x, y) = (((mouse.x - frame.origin.x) * scale) as f32, ((frame.origin.y + frame.size.height - mouse.y) * scale) as f32);
    let opaque = inside && app.buddy.opaque_at(x, y);
    if opaque {
        let cursor = if app.buddy.hit_hand(x, y) { NSCursor::pointingHandCursor() } else { NSCursor::arrowCursor() };
        cursor.set();
    }
    let ignore = !opaque;
    if app.ignoring != Some(ignore) {
        app.window.setIgnoresMouseEvents(ignore);
        app.ignoring = Some(ignore);
    }
}

fn status_icon() -> Option<Retained<NSImage>> {
    let dir = crate::sprites::find_pose_dir()?;
    let source = tiny_skia::Pixmap::load_png(dir.join("idle-0.png")).ok()?;
    let side = (source.width() as f32 * 0.66) as u32;
    let rect = tiny_skia::IntRect::from_xywh(((source.width() - side) / 2) as i32, (source.height() as f32 * 0.02) as i32, side, side)?;
    let icon = crate::sprites::area_downscale(&source.clone_rect(rect)?, 44, 44);
    let image = cg_image(icon.data(), 44, 44)?;
    Some(NSImage::initWithCGImage_size(NSImage::alloc(), &image, NSSize::new(22.0, 22.0)))
}

// ---- placement ----
// buddy-position.txt holds the top-left corner in points, y counted down from the top of
// the primary screen, so it survives the bottom-left origin AppKit uses.

fn primary_height() -> f64 {
    NSScreen::screens(mtm()).firstObject().map_or(900.0, |s| s.frame().size.height)
}

fn visible_frame_at(point: NSPoint) -> NSRect {
    let screens = NSScreen::screens(mtm());
    for screen in screens.iter() {
        let f = screen.frame();
        if point.x >= f.origin.x && point.x < f.origin.x + f.size.width && point.y >= f.origin.y && point.y < f.origin.y + f.size.height {
            return screen.visibleFrame();
        }
    }
    NSScreen::mainScreen(mtm()).map_or(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 800.0)), |s| s.visibleFrame())
}

fn clamp_origin(origin: NSPoint, size: NSSize) -> NSPoint {
    let area = visible_frame_at(NSPoint::new(origin.x + size.width / 2.0, origin.y + size.height / 2.0));
    let max_x = (area.origin.x + area.size.width - size.width).max(area.origin.x);
    let max_y = (area.origin.y + area.size.height - size.height).max(area.origin.y);
    NSPoint::new(origin.x.clamp(area.origin.x, max_x), origin.y.clamp(area.origin.y, max_y))
}

fn initial_origin(path: &Path, size: NSSize) -> NSPoint {
    if let Ok(text) = std::fs::read_to_string(path) {
        let parts: Vec<f64> = text.trim().split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if parts.len() == 2 {
            let origin = NSPoint::new(parts[0], primary_height() - parts[1] - size.height);
            return clamp_origin(origin, size);
        }
    }
    let area = visible_frame_at(NSEvent::mouseLocation());
    NSPoint::new(area.origin.x + area.size.width - size.width - 16.0, area.origin.y + 16.0)
}

fn save_position(app: &App) {
    let frame = app.window.frame();
    let top = primary_height() - (frame.origin.y + frame.size.height);
    let _ = std::fs::write(&app.position_path, format!("{},{}", frame.origin.x.round(), top.round()));
}

// ---- input ----

fn handle_event(ev: &NSEvent) -> bool {
    let ours = with_app(|app| ev.windowNumber() == app.window.windowNumber()).unwrap_or(false);
    if !ours {
        return false;
    }
    match ev.r#type() {
        NSEventType::LeftMouseDown => {
            let grab = NSEvent::mouseLocation();
            with_app(|app| app.drag = Some(Drag { grab, start: app.window.frame().origin, moved: false }));
            true
        }
        NSEventType::LeftMouseDragged => {
            let now = NSEvent::mouseLocation();
            with_app(|app| {
                let Some(drag) = app.drag.as_mut() else { return };
                let (dx, dy) = (now.x - drag.grab.x, now.y - drag.grab.y);
                if !drag.moved && dx.abs() + dy.abs() > 4.0 {
                    drag.moved = true;
                }
                if drag.moved {
                    app.window.setFrameOrigin(NSPoint::new(drag.start.x + dx, drag.start.y + dy));
                }
            });
            true
        }
        NSEventType::LeftMouseUp => {
            let at = ev.locationInWindow();
            let action = with_app(|app| {
                let drag = app.drag.take()?;
                if drag.moved {
                    let frame = app.window.frame();
                    app.window.setFrameOrigin(clamp_origin(frame.origin, frame.size));
                    save_position(app);
                    return None;
                }
                let frame = app.window.frame();
                let scale = app.window.backingScaleFactor();
                Some(app.buddy.click((at.x * scale) as f32, ((frame.size.height - at.y) * scale) as f32))
            })
            .flatten();
            match action {
                Some(Click::Dismiss) => close(),
                Some(Click::Focus(session)) => show_session_window(&session),
                _ => {}
            }
            on_tick();
            true
        }
        NSEventType::RightMouseUp => {
            pop_up_menu();
            true
        }
        _ => false,
    }
}

// ---- menus ----

fn build_menu(entries: &[Entry], target: &MenuTarget) -> Retained<NSMenu> {
    let mtm = mtm();
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let receiver: &AnyObject = target;
    for entry in entries {
        match entry {
            Entry::Item { id, label, checked, enabled, bold } => {
                let title = if *bold { format!("★ {label}") } else { label.clone() };
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(&title), Some(sel!(pick:)), ns_string!(""))
                };
                unsafe { item.setTarget(Some(receiver)) };
                item.setTag(*id as isize);
                if *checked {
                    item.setState(NSControlStateValueOn);
                }
                item.setEnabled(*enabled);
                menu.addItem(&item);
            }
            Entry::Sub { label, items } => {
                let parent = NSMenuItem::new(mtm);
                parent.setTitle(&NSString::from_str(label));
                parent.setSubmenu(Some(&build_menu(items, target)));
                menu.addItem(&parent);
            }
            Entry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
        }
    }
    menu
}

/// The menu bar item holds a ready menu; rebuild it when its contents would change.
fn refresh_status_menu(app: &mut App) {
    if app.menu_checked.is_some_and(|at| at.elapsed() < Duration::from_secs(1)) {
        return;
    }
    app.menu_checked = Some(Instant::now());
    let entries = menu::entries(&app.buddy);
    let signature = format!("{entries:?}");
    if signature != app.menu_shown {
        app.status.setMenu(Some(&build_menu(&entries, &app.target)));
        app.menu_shown = signature;
    }
}

fn pop_up_menu() {
    let Some(menu) = with_app(|app| build_menu(&menu::entries(&app.buddy), &app.target)) else { return };
    // Blocks until the menu closes; picking an item calls on_menu meanwhile, so no borrow is held.
    menu.popUpMenuPositioningItem_atLocation_inView(None, NSEvent::mouseLocation(), None);
}

fn on_menu(cmd: usize) {
    let action = with_app(|app| {
        let action = menu::apply(&mut app.buddy, cmd);
        if matches!(action, Action::Changed) {
            app.buddy.dark = wants_dark(&app.buddy.state.settings.theme);
            app.menu_checked = None;
        }
        action
    });
    match action {
        Some(Action::Quit) => quit(),
        Some(Action::Open(url)) => open_url(&url),
        _ => on_tick(),
    }
}

// ---- desktop integration ----

fn play_sound(kind: &str) {
    let name = match kind {
        "error" => "Basso",
        "done" => "Glass",
        _ => "Funk",
    };
    if let Some(sound) = NSSound::soundNamed(&NSString::from_str(name)) {
        sound.play();
    }
}

/// Window titles need Screen Recording permission, so only the app is known.
fn foreground() -> Option<Foreground> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let process = app.localizedName().map(|n| n.to_string()).unwrap_or_default();
    Some(Foreground { process, title: String::new(), window: app.processIdentifier().to_string() })
}

/// `open -a Cursor <folder>` brings that folder's window forward, or opens it.
fn show_session_window(s: &Session) {
    let mut cmd = std::process::Command::new("/usr/bin/open");
    cmd.args(["-a", "Cursor"]);
    if !s.workspace_root.is_empty() {
        cmd.arg(&s.workspace_root);
    }
    let _ = cmd.spawn();
}

fn wants_dark(theme: &str) -> bool {
    match theme {
        "dark" => true,
        "light" => false,
        _ => NSUserDefaults::standardUserDefaults()
            .stringForKey(ns_string!("AppleInterfaceStyle"))
            .is_some_and(|v| v.to_string() == "Dark"),
    }
}

fn open_url(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    if let Some(u) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&u);
    }
}
