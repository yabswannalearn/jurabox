use super::*;
use std::{cell::RefCell, rc::Rc, time::Duration};

pub(super) fn run() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let started = Instant::now();
    let (window, _tray) = create(ProjectStore::new(temporary.path().join("projects.json")))?;
    window.show()?;
    let initialized_ms = started.elapsed().as_secs_f64() * 1000.;
    let baseline = crate::platform::metrics::sample(std::process::id())?;
    let result = Rc::new(RefCell::new(None));
    let outcome = result.clone();
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    let mut step = 0;
    let mut visible_cpu = 0.;
    let mut hidden_start = baseline.cpu_seconds;
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(500), move || {
        let Some(window) = weak.upgrade() else { return; };
        let sample = match crate::platform::metrics::sample(std::process::id()) { Ok(sample) => sample,
            Err(error) => { *outcome.borrow_mut() = Some(Err(error)); let _ = slint::quit_event_loop(); return; } };
        if step == 3 { visible_cpu = sample.cpu_seconds - baseline.cpu_seconds; hidden_start = sample.cpu_seconds; let _ = window.hide(); }
        if step == 9 {
            *outcome.borrow_mut() = Some(Ok(serde_json::json!({"debug_build":cfg!(debug_assertions), "initialized_ms":initialized_ms,
                "visible_sample_seconds":2,"visible_cpu_seconds":visible_cpu,"hidden_sample_seconds":3,
                "hidden_cpu_seconds":sample.cpu_seconds-hidden_start,"final_process":sample,
                "note":"Private empty settings; includes app catalog/background setup. Initialization excludes first frame."})));
            let _ = slint::quit_event_loop();
        }
        step += 1;
    });
    slint::run_event_loop_until_quit()?;
    timer.stop();
    let output = result
        .borrow_mut()
        .take()
        .context("UI performance check did not finish")??;
    std::fs::create_dir_all("target/performance-check")?;
    std::fs::write(
        "target/performance-check/ui.json",
        serde_json::to_vec_pretty(&output)?,
    )?;
    println!("UI performance: {output}");
    Ok(())
}
