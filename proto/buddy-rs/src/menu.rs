// The right-click / tray menu, shared by the Windows and macOS layers.
use crate::buddy::Buddy;
use crate::lang::t;

#[derive(Debug, PartialEq)]
pub enum Entry {
    Item { id: usize, label: String, checked: bool, enabled: bool, bold: bool },
    Sub { label: String, items: Vec<Entry> },
    Separator,
}

pub enum Action {
    Nothing,
    /// A setting changed and was saved; the platform should re-read the theme.
    Changed,
    Quit,
    Open(String),
}

const MUTE: usize = 1;
const QUIET: usize = 2;
const QUOTA: usize = 3;
const UPDATES: usize = 4;
const USAGE_PAGE: usize = 5;
const NEW_VERSION: usize = 6;
const THEME: usize = 10;
const LANGUAGE: usize = 20;
const SECONDS: usize = 30;
const STALE: usize = 40;
const QUIT: usize = 99;
const THEMES: [&str; 3] = ["auto", "light", "dark"];
const LANGUAGES: [&str; 3] = ["auto", "zh", "en"];
const SECOND_CHOICES: [i64; 4] = [6, 12, 20, 30];
const STALE_CHOICES: [i64; 4] = [30, 60, 180, 360];

fn item(id: usize, label: String, checked: bool) -> Entry {
    Entry::Item { id, label, checked, enabled: true, bold: false }
}

fn follow_system() -> String {
    if cfg!(target_os = "macos") {
        t("跟随系统", "Follow macOS")
    } else {
        t("跟随系统", "Follow Windows")
    }
}

pub fn entries(buddy: &Buddy) -> Vec<Entry> {
    let st = &buddy.state;
    let s = &st.settings;
    let mut out = Vec::new();
    if let Some(u) = buddy.update.as_ref().filter(|u| s.updates && u.newer()) {
        let label = t(&format!("发现新版本 {}，点这里下载", u.tag), &format!("New version {} - click to download", u.tag));
        out.push(Entry::Item { id: NEW_VERSION, label, checked: false, enabled: true, bold: true });
    }
    out.push(item(MUTE, t("静音", "Mute"), s.mute));
    out.push(item(QUIET, t("正看着对话时不打扰", "Stay quiet while I'm watching"), s.quiet));
    out.push(Entry::Item {
        id: QUOTA,
        label: t("显示额度栏", "Show usage bar"),
        checked: buddy.has_quota() && s.quota,
        enabled: buddy.has_quota(),
        bold: false,
    });

    let theme_names = [follow_system(), t("浅色", "Light"), t("深色", "Dark")];
    let items = (0..3).map(|i| item(THEME + i, theme_names[i].clone(), s.theme == THEMES[i])).collect();
    out.push(Entry::Sub { label: t("主题", "Theme"), items });

    let items = SECOND_CHOICES
        .iter()
        .enumerate()
        .map(|(i, n)| item(SECONDS + i, t(&format!("{n} 秒"), &format!("{n} seconds")), st.done_seconds == *n))
        .collect();
    out.push(Entry::Sub { label: t("结束后停留", "Stay after finishing"), items });

    let items = STALE_CHOICES
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let label = if *n < 60 {
                t(&format!("{n} 分钟"), &format!("{n} minutes"))
            } else {
                let hours = n / 60;
                t(&format!("{hours} 小时"), &format!("{hours} {}", if hours == 1 { "hour" } else { "hours" }))
            };
            item(STALE + i, label, st.stale_minutes == *n)
        })
        .collect();
    out.push(Entry::Sub { label: t("多久没动静算失联", "Treat as lost after"), items });

    let language_names = [follow_system(), "中文".to_string(), "English".to_string()];
    let items = (0..3).map(|i| item(LANGUAGE + i, language_names[i].clone(), s.language == LANGUAGES[i])).collect();
    out.push(Entry::Sub { label: t("语言", "Language"), items });

    out.push(item(UPDATES, t("自动检查更新", "Check for updates"), s.updates));
    out.push(item(USAGE_PAGE, t("打开 Cursor 用量页", "Open Cursor usage page"), false));
    out.push(Entry::Separator);
    out.push(item(QUIT, t("先关掉点点", "Close for now"), false));
    out
}

pub fn apply(buddy: &mut Buddy, cmd: usize) -> Action {
    match cmd {
        QUIT => return Action::Quit,
        USAGE_PAGE => return Action::Open("https://cursor.com/dashboard?tab=usage".into()),
        NEW_VERSION => return buddy.update.as_ref().map_or(Action::Nothing, |u| Action::Open(u.url.clone())),
        _ => {}
    }
    let st = &mut buddy.state.settings;
    match cmd {
        MUTE => st.mute = !st.mute,
        QUIET => st.quiet = !st.quiet,
        QUOTA => st.quota = !st.quota,
        UPDATES => st.updates = !st.updates,
        c if (THEME..THEME + 3).contains(&c) => st.theme = THEMES[c - THEME].into(),
        c if (LANGUAGE..LANGUAGE + 3).contains(&c) => st.language = LANGUAGES[c - LANGUAGE].into(),
        c if (SECONDS..SECONDS + 4).contains(&c) => st.seconds = SECOND_CHOICES[c - SECONDS],
        c if (STALE..STALE + 4).contains(&c) => st.stale_minutes = STALE_CHOICES[c - STALE],
        _ => return Action::Nothing,
    }
    buddy.save_settings();
    Action::Changed
}
