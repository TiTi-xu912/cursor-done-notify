#![cfg_attr(windows, windows_subsystem = "windows")]
// Only Windows and macOS have a window layer; elsewhere the host core is just compiled and tested.
#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

mod buddy;
mod clock;
mod event;
mod jsonscan;
mod lang;
#[cfg(target_os = "macos")]
mod mac;
mod menu;
mod net;
mod quota;
mod render;
mod sprites;
mod state;
#[cfg(test)]
mod tests;
mod text;
mod update;
#[cfg(windows)]
mod win;

use std::path::PathBuf;
use std::time::Duration;

pub struct Options {
    /// Hide instead of exiting when every turn is done, and come back on the next one.
    pub stay: bool,
    /// Quit after this long no matter what; used by the CI smoke test.
    pub exit_after: Option<Duration>,
}

pub fn state_dir() -> PathBuf {
    std::env::temp_dir().join("cursor-done-notify")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--fonts") {
        match text::Fonts::load() {
            Some(fonts) => println!("{}", fonts.describe()),
            None => println!("no fonts found"),
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--snapshot") {
        let dir = args.get(i + 1).map(PathBuf::from).unwrap_or_else(|| state_dir().join("preview-rs"));
        if let Err(e) = buddy::snapshots(&dir) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    let exit_after = args
        .iter()
        .position(|a| a == "--exit-after")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs);
    let opts = Options { stay: args.iter().any(|a| a == "--stay"), exit_after };
    #[cfg(windows)]
    win::run(opts);
    #[cfg(target_os = "macos")]
    mac::run(opts);
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = opts;
        eprintln!("这个平台还没有窗口层；可以用 --snapshot <目录> 只测绘制。");
    }
}
