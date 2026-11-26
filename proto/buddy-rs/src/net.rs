pub struct NetError {
    /// HTTP status when the server answered, None for transport failures.
    pub status: Option<u32>,
    pub message: String,
}

pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    pub body: Option<&'a str>,
    pub timeout_ms: u32,
}

#[cfg(windows)]
pub fn send(req: &Request) -> Result<String, NetError> {
    win::send(req)
}

#[cfg(not(windows))]
pub fn send(req: &Request) -> Result<String, NetError> {
    curl::send(req)
}

fn fail(message: impl Into<String>) -> NetError {
    NetError { status: None, message: message.into() }
}

#[cfg(windows)]
mod win {
    use super::{fail, NetError, Request};
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Networking::WinHttp::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Handle(*mut c_void);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }

    fn last_error(what: &str) -> NetError {
        fail(format!("{what} (WinHTTP {})", unsafe { GetLastError() }))
    }

    pub fn send(req: &Request) -> Result<String, NetError> {
        let rest = req.url.strip_prefix("https://").ok_or_else(|| fail("only https is supported"))?;
        let (host, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
        unsafe {
            let agent = wide("cursor-done-notify");
            let session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0));
            if session.0.is_null() {
                return Err(last_error("WinHttpOpen"));
            }
            let t = req.timeout_ms as i32;
            WinHttpSetTimeouts(session.0, t, t, t, t);
            let host_w = wide(host);
            let connect = Handle(WinHttpConnect(session.0, host_w.as_ptr(), 443, 0));
            if connect.0.is_null() {
                return Err(last_error("connect"));
            }
            let verb = wide(req.method);
            let path_w = wide(&path);
            let request = Handle(WinHttpOpenRequest(connect.0, verb.as_ptr(), path_w.as_ptr(), null(), null(), null(), WINHTTP_FLAG_SECURE));
            if request.0.is_null() {
                return Err(last_error("open request"));
            }
            let headers: String = req.headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
            let headers_w: Vec<u16> = headers.encode_utf16().collect();
            let body = req.body.unwrap_or("").as_bytes();
            let ok = WinHttpSendRequest(
                request.0,
                if headers_w.is_empty() { null() } else { headers_w.as_ptr() },
                headers_w.len() as u32,
                if body.is_empty() { null() } else { body.as_ptr() as *const c_void },
                body.len() as u32,
                body.len() as u32,
                0,
            );
            if ok == 0 || WinHttpReceiveResponse(request.0, null_mut()) == 0 {
                return Err(last_error("request failed"));
            }
            let mut status = 0u32;
            let mut size = 4u32;
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                null(),
                &mut status as *mut u32 as *mut c_void,
                &mut size,
                null_mut(),
            );
            let mut out = Vec::new();
            let mut chunk = vec![0u8; 16 * 1024];
            loop {
                let mut read = 0u32;
                if WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read) == 0 {
                    return Err(last_error("read failed"));
                }
                if read == 0 {
                    break;
                }
                out.extend_from_slice(&chunk[..read as usize]);
            }
            let text = String::from_utf8_lossy(&out).into_owned();
            if !(200..300).contains(&status) {
                return Err(NetError { status: Some(status), message: format!("HTTP {status}") });
            }
            Ok(text)
        }
    }
}

#[cfg(not(windows))]
mod curl {
    use super::{fail, NetError, Request};
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Headers go through a curl config on stdin so the token never shows up in `ps`.
    pub fn send(req: &Request) -> Result<String, NetError> {
        let mut config = String::new();
        config.push_str(&format!("url = \"{}\"\nrequest = \"{}\"\n", req.url, req.method));
        for (k, v) in req.headers {
            config.push_str(&format!("header = \"{}: {}\"\n", k, v.replace('"', "")));
        }
        if let Some(body) = req.body {
            config.push_str(&format!("data-raw = \"{}\"\n", body.replace('\\', "\\\\").replace('"', "\\\"")));
        }
        let secs = (req.timeout_ms / 1000).max(1);
        let mut child = Command::new("/usr/bin/curl")
            .args(["-sS", "--max-time", &secs.to_string(), "-w", "\n%{http_code}", "-K", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| fail(format!("curl: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(config.as_bytes());
        }
        let output = child.wait_with_output().map_err(|e| fail(format!("curl: {e}")))?;
        if !output.status.success() {
            return Err(fail(String::from_utf8_lossy(&output.stderr).trim().to_string()));
        }
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let (body, code) = text.rsplit_once('\n').unwrap_or((&text, "0"));
        let status: u32 = code.trim().parse().unwrap_or(0);
        if !(200..300).contains(&status) {
            return Err(NetError { status: Some(status), message: format!("HTTP {status}") });
        }
        Ok(body.to_string())
    }
}
