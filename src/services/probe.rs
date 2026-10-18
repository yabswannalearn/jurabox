use crate::projects::Readiness;
use anyhow::{Context, Result, ensure};
use std::{
    net::TcpStream,
    sync::mpsc,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(super) struct Probe {
    pub attempt: Uuid,
    pub preflight: bool,
    receiver: mpsc::Receiver<(Instant, std::result::Result<bool, String>)>,
}

impl Probe {
    pub fn start(attempt: Uuid, condition: Readiness, preflight: bool) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("service-readiness".into())
            .spawn(move || {
                let result = match condition {
                    Readiness::ProcessRunning => Ok(true),
                    Readiness::Tcp { address } => Ok(TcpStream::connect_timeout(
                        &address,
                        Duration::from_millis(300),
                    )
                    .is_ok()),
                    Readiness::Http { url } => {
                        http_status(&url).map(|status| preflight || (200..300).contains(&status))
                    }
                }
                .map_err(|error| format!("{error:#}"));
                let _ = sender.send((Instant::now(), result));
            })
            .context("Cannot start the readiness check.")?;
        Ok(Self {
            attempt,
            preflight,
            receiver,
        })
    }

    pub fn result(&self) -> Option<(Instant, std::result::Result<bool, String>)> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some((Instant::now(), Err("Readiness worker stopped.".into())))
            }
        }
    }
}

fn http_status(value: &str) -> Result<u32> {
    use windows::{
        Win32::Networking::WinHttp::*,
        core::{HSTRING, PCWSTR, w},
    };
    struct Internet(*mut std::ffi::c_void);
    impl Internet {
        fn new(value: *mut std::ffi::c_void) -> Result<Self> {
            if value.is_null() {
                return Err(windows::core::Error::from_win32().into());
            }
            Ok(Self(value))
        }
    }
    impl Drop for Internet {
        fn drop(&mut self) {
            // SAFETY: This uniquely owned WinHTTP handle is closed exactly once.
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
    let url = url::Url::parse(value)?;
    let host = HSTRING::from(
        url.host_str()
            .context("Health URL has no host")?
            .trim_matches(['[', ']']),
    );
    let mut path = url.path().to_owned();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let path = HSTRING::from(path);
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "Health URLs require HTTP(S)."
    );
    // SAFETY: Handles and UTF-16 strings stay alive through each synchronous call.
    // Network work occurs on a dedicated bounded-time worker, never the UI or
    // workspace actor. Only response status is read; response bodies are ignored.
    unsafe {
        let session = Internet::new(WinHttpOpen(
            w!("Jurabox readiness/0.1"),
            WINHTTP_ACCESS_TYPE_NO_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 1000, 500, 500, 500)?;
        let connection = Internet::new(WinHttpConnect(
            session.0,
            &host,
            url.port_or_known_default()
                .context("Health URL has no port")?,
            0,
        ))?;
        let flags = if url.scheme() == "https" {
            WINHTTP_FLAG_SECURE
        } else {
            WINHTTP_OPEN_REQUEST_FLAGS(0)
        };
        let request = Internet::new(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            &path,
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        ))?;
        let disabled =
            (WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_AUTHENTICATION)
                .to_ne_bytes();
        WinHttpSetOption(
            Some(request.0),
            WINHTTP_OPTION_DISABLE_FEATURE,
            Some(&disabled),
        )?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())?;
        let mut status = 0u32;
        let mut length = std::mem::size_of_val(&status) as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut length,
            std::ptr::null_mut(),
        )?;
        Ok(status)
    }
}
