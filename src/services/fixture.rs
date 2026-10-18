use anyhow::{Result, ensure};
use std::{
    fs,
    io::Write,
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
        "success" => {
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
        "long" | "flaky" | "flood" => {}
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
    while start.elapsed() < Duration::from_secs(60) && !folder.join("stop").exists() {
        thread::sleep(Duration::from_millis(25));
    }
    println!("Fixture stopped.");
    Ok(())
}
