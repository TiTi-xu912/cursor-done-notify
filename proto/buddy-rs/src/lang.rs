use std::cell::Cell;

// Per thread so unit tests can switch language without racing each other;
// the app itself only touches it from the UI thread.
thread_local! {
    static ENGLISH: Cell<bool> = const { Cell::new(false) };
}

pub fn english() -> bool {
    ENGLISH.with(|e| e.get())
}

pub fn set_english(on: bool) {
    ENGLISH.with(|e| e.set(on));
}

pub fn t(zh: &str, en: &str) -> String {
    if english() { en.to_string() } else { zh.to_string() }
}

/// "zh" / "en" force a language; anything else follows the system UI language.
pub fn apply(setting: &str) {
    set_english(setting == "en" || (setting != "zh" && !system_is_chinese()));
}

#[cfg(windows)]
fn system_is_chinese() -> bool {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    // Primary language id 0x04 is Chinese in every region.
    (unsafe { GetUserDefaultUILanguage() } & 0x3ff) == 0x04
}

#[cfg(not(windows))]
fn system_is_chinese() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .map(|v| v.starts_with("zh"))
        .unwrap_or(false)
}
