use std::time::{SystemTime, UNIX_EPOCH};

/// Wall-clock time in milliseconds since the Unix epoch (UTC).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp(pub i64);

impl Stamp {
    pub fn now() -> Stamp {
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        Stamp(ms)
    }

    pub fn add_secs(self, secs: f64) -> Stamp {
        Stamp(self.0 + (secs * 1000.0) as i64)
    }

    pub fn add_mins(self, mins: f64) -> Stamp {
        self.add_secs(mins * 60.0)
    }

    pub fn secs_since(self, earlier: Stamp) -> f64 {
        (self.0 - earlier.0) as f64 / 1000.0
    }

    /// Local "HH:mm".
    pub fn local_hm(self) -> String {
        let (_, _, h, m) = local_parts(self.0);
        format!("{h:02}:{m:02}")
    }

    /// Local "MM/dd".
    pub fn local_md(self) -> String {
        let (mo, d, _, _) = local_parts(self.0);
        format!("{mo:02}/{d:02}")
    }

    /// .NET DateTime ticks (UTC), the unit the C# host writes into shared cache files.
    pub fn to_dotnet_ticks(self) -> i64 {
        self.0 * 10_000 + DOTNET_TICKS_AT_EPOCH
    }

    pub fn from_dotnet_ticks(ticks: i64) -> Option<Stamp> {
        (ticks > DOTNET_TICKS_AT_EPOCH).then(|| Stamp((ticks - DOTNET_TICKS_AT_EPOCH) / 10_000))
    }
}

const DOTNET_TICKS_AT_EPOCH: i64 = 621_355_968_000_000_000;

/// (month, day, hour, minute) in local time.
#[cfg(windows)]
fn local_parts(ms: i64) -> (u32, u32, u32, u32) {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
    use windows_sys::Win32::System::Time::FileTimeToSystemTime;
    // FILETIME counts 100 ns ticks since 1601-01-01.
    let ticks = (ms as u64 + 11_644_473_600_000) * 10_000;
    let utc = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let mut local = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe {
        if FileTimeToLocalFileTime(&utc, &mut local) == 0 || FileTimeToSystemTime(&local, &mut st) == 0 {
            return (0, 0, 0, 0);
        }
    }
    (st.wMonth as u32, st.wDay as u32, st.wHour as u32, st.wMinute as u32)
}

#[cfg(unix)]
fn local_parts(ms: i64) -> (u32, u32, u32, u32) {
    let secs = (ms / 1000) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return (0, 0, 0, 0);
    }
    (tm.tm_mon as u32 + 1, tm.tm_mday as u32, tm.tm_hour as u32, tm.tm_min as u32)
}

/// "mm:ss", or "h:mm:ss" past an hour; never rounds up.
pub fn elapsed(start: Option<Stamp>, now: Stamp) -> String {
    let Some(start) = start else { return "00:00".to_string() };
    let total = ((now.0 - start.0).max(0) / 1000) as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h >= 1 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{:02}:{s:02}", total / 60)
    }
}
