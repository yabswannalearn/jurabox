use anyhow::{Context, Result};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// Optional real-browser fixture: serves local pages and records each requested
/// path. The server exists only during the explicitly requested desktop exercise.
pub(super) struct BrowserPages {
    pub urls: Vec<String>,
    requested: Arc<AtomicU8>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl BrowserPages {
    pub fn start() -> Result<Self> {
        let listener =
            TcpListener::bind("127.0.0.1:0").context("Cannot start local browser test pages")?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let requested = Arc::new(AtomicU8::new(0));
        let worker_stop = stop.clone();
        let worker_requested = requested.clone();
        let thread = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut connection, _)) => {
                        let _ = connection.set_read_timeout(Some(Duration::from_millis(500)));
                        let _ = connection.set_write_timeout(Some(Duration::from_millis(500)));
                        let mut request = [0; 4096];
                        if let Ok(length) = connection.read(&mut request) {
                            let request = String::from_utf8_lossy(&request[..length]);
                            if request.starts_with("GET /one ") {
                                worker_requested.fetch_or(1, Ordering::Relaxed);
                            }
                            if request.starts_with("GET /two ") {
                                worker_requested.fetch_or(2, Ordering::Relaxed);
                            }
                            let body = "<!doctype html><title>Jurabox browser check</title><h1>Jurabox browser check</h1><p>This temporary local page will close when the check finishes.</p>";
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            let _ = connection.write_all(response.as_bytes());
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            urls: vec![
                format!("http://{address}/one"),
                format!("http://{address}/two"),
            ],
            requested,
            stop,
            thread: Some(thread),
        })
    }

    pub fn both_loaded(&self) -> bool {
        self.requested.load(Ordering::Relaxed) == 3
    }
}

impl Drop for BrowserPages {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
