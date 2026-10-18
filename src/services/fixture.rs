use anyhow::{Result, ensure};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

/// Finite, diagnostic-only workload. It touches only the explicitly supplied
/// existing folder and never selects another process to stop.
pub fn run(mode: &str, folder: &Path) -> Result<()> {
    ensure!(
        folder.is_absolute() && folder.is_dir(),
        "Fixture folder must already exist."
    );
    match mode {
        "stop" => {
            fs::write(folder.join("stop"), "stop")?;
            return Ok(());
        }
        "success" | "delayed-success" => {
            let start = Instant::now();
            while mode == "delayed-success"
                && !folder.join("allow-action").exists()
                && start.elapsed() < Duration::from_secs(30)
            {
                thread::sleep(Duration::from_millis(25));
            }
            ensure!(
                mode != "delayed-success" || folder.join("allow-action").exists(),
                "Action fixture gate timed out"
            );
            fs::write(folder.join("action-completed"), "completed")?;
            println!("Action finished successfully.");
            return Ok(());
        }
        "fail" => anyhow::bail!("Intentional fixture failure"),
        "flaky" if !folder.join("attempted").exists() => {
            fs::write(folder.join("attempted"), "first attempt")?;
            anyhow::bail!("Intentional first-attempt failure");
        }
        "parent-exit" => {
            let child = Command::new(std::env::current_exe()?)
                .arg("_service-fixture")
                .arg("long")
                .arg(folder)
                .creation_flags(0x08000000)
                .spawn()?;
            fs::write(folder.join("child-pid"), child.id().to_string())?;
            println!(
                "Parent exiting; child {} remains in the workload.",
                child.id()
            );
            return Ok(());
        }
        "long" | "flaky" | "flood" | "health" => {}
        _ => anyhow::bail!("Unknown service fixture mode"),
    }
    let _ = fs::remove_file(folder.join("stop"));
    fs::write(
        folder.join(format!("{}.started", std::process::id())),
        std::env::current_dir()?.to_string_lossy().as_bytes(),
    )?;
    if mode == "flood" {
        std::io::stdout().write_all("测试-output-".repeat(12000).as_bytes())?;
    }
    println!(
        "\nFixture ready; pid={}, folder={}",
        std::process::id(),
        folder.display()
    );
    std::io::stdout().flush()?;
    let start = Instant::now();
    let mut listener = None;
    while start.elapsed() < Duration::from_secs(60) && !folder.join("stop").exists() {
        if mode == "health" && folder.join("allow-ready").exists() {
            if listener.is_none() {
                let address = fs::read_to_string(folder.join("listen-address"))?;
                let server = TcpListener::bind(address.trim())?;
                server.set_nonblocking(true)?;
                listener = Some(server);
            }
            if let Some(server) = &listener {
                match server.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_read_timeout(Some(Duration::from_millis(75)))?;
                        stream.set_write_timeout(Some(Duration::from_millis(75)))?;
                        let mut buffer = [0u8; 4096];
                        if let Ok(count) = stream.read(&mut buffer) {
                            let request = String::from_utf8_lossy(&buffer[..count]);
                            if request.starts_with("GET ") {
                                fs::write(folder.join("last-request"), request.as_bytes())?;
                                let status = if request.starts_with("GET /healthy ") {
                                    "204".into()
                                } else {
                                    fs::read_to_string(folder.join("http-status"))
                                        .unwrap_or_else(|_| "503".into())
                                };
                                let response = format!(
                                    "HTTP/1.1 {} Fixture\r\nLocation: /healthy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                    status.trim()
                                );
                                let _ = stream.write_all(response.as_bytes());
                            }
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    if let Ok(prerequisite) = fs::read_to_string(folder.join("require-live-folder")) {
        ensure!(
            !Path::new(prerequisite.trim()).join("stop").exists(),
            "Prerequisite was stopped before its dependent"
        );
    }
    fs::write(folder.join("stopped"), "stopped")?;
    println!("Fixture stopped.");
    Ok(())
}
