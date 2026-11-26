#![cfg_attr(windows, windows_subsystem = "windows")]
// Until the macOS window layer exists, nothing outside Windows drives the host core.
#![cfg_attr(not(windows), allow(dead_code))]

mod buddy;
mod clock;
mod event;
mod jsonscan;
mod lang;
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

pub struct Options {
    /// Hide instead of exiting when every turn is done, and come back on the next one.
    pub stay: bool,
}

pub fn state_dir() -> PathBuf {
    std::env::temp_dir().join("cursor-done-notify")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--snapshot") {
        let dir = args.get(i + 1).map(PathBuf::from).unwrap_or_else(|| state_dir().join("preview-rs"));
        if let Err(e) = buddy::snapshots(&dir) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    let opts = Options { stay: args.iter().any(|a| a == "--stay") };
    #[cfg(windows)]
    win::run(opts);
    #[cfg(not(windows))]
    {
        let _ = opts;
        eprintln!("这个原型目前只有 Windows 平台层；可以用 --snapshot <目录> 只测绘制。");
    }
}
