use super::*;
use anyhow::{bail, ensure};
use slint::Model;
use std::{cell::RefCell, fs::File, io::BufWriter, path::Path, rc::Rc, time::Duration};

pub(super) fn snapshot(window: &AppWindow, path: &Path) -> Result<()> {
    let pixels = window.window().take_snapshot()?;
    let mut encoder = png::Encoder::new(
        BufWriter::new(File::create(path)?),
        pixels.width(),
        pixels.height(),
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()?
        .write_image_data(pixels.as_bytes())?;
    Ok(())
}

#[cfg(windows)]
fn native_tray_click() -> Result<()> {
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, WPARAM},
            UI::WindowsAndMessaging::{
                FindWindowExW, GetWindowThreadProcessId, HWND_MESSAGE, PostMessageW, WM_APP,
                WM_LBUTTONUP,
            },
        },
        core::w,
    };
    // Slint 1.18.1 uses this message-only class and callback message. This is
    // deliberately a version-specific smoke check, not a production tray dependency.
    // SAFETY: Search only message-only windows, validate the owning process, and
    // send a click notification only to this process's own Slint tray window.
    unsafe {
        let mut after = HWND::default();
        loop {
            let hwnd = FindWindowExW(HWND_MESSAGE, after, w!("SlintSystemTrayWindow"), None)
                .context("Slint did not register its native tray window")?;
            let mut process = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut process));
            if process == std::process::id() {
                PostMessageW(hwnd, WM_APP + 1, WPARAM(1), LPARAM(WM_LBUTTONUP as isize))?;
                return Ok(());
            }
            after = hwnd;
        }
    }
}

#[cfg(not(windows))]
fn native_tray_click() -> Result<()> {
    bail!("the live UI check currently requires Windows")
}

pub fn run() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let settings = temporary.path().join("settings");
    let backup = temporary.path().join("settings-backup");
    let path = settings.join("projects.json");
    let folder = temporary.path().join("project-files");
    std::fs::create_dir(&folder)?;
    std::fs::write(folder.join("work.txt"), "preserve this file")?;
    let artifacts = std::env::current_dir()?.join("target/ui-check");
    std::fs::create_dir_all(&artifacts)?;
    let (window, tray) = create(ProjectStore::new(path.clone()))?;
    #[cfg(windows)]
    let instance_name = format!("Jurabox.UICheck.{}", Uuid::new_v4());
    #[cfg(windows)]
    let mut instance = crate::platform::ManagerInstance::acquire_named(&instance_name)?
        .context("UI check instance name was already in use")?;
    #[cfg(windows)]
    instance.listen(window.as_weak());
    window.show()?;
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    let result = Rc::new(RefCell::new(None));
    let outcome = result.clone();
    let mut step = 0;
    let mut original_id = None;
    let mut selected_windows_app = None;
    let started = Instant::now();
    let timeout = Duration::from_secs(60);
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
            let mut run_step = || -> Result<bool> {
            ensure!(started.elapsed() < timeout, "UI check timed out at step {step}");
            let window = weak.upgrade().context("UI window disappeared")?;
            if window.get_busy() { return Ok(false); }
            let read = || ProjectStore::new(path.clone()).load();
            match step {
                0 => {
                    ensure!(window.window().is_visible(), "main window did not open");
                    snapshot(&window, &artifacts.join("empty.png"))?;
                    window.invoke_save_project();
                    ensure!(window.get_notice_error(), "empty project name was accepted");
                    window.invoke_focus_name();
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::KeyPressed { text: "Jurabox".into() })?;
                    ensure!(window.get_project_name() == "Jurabox" && window.get_dirty(), "keyboard editing did not update the form and dirty state");
                    window.set_project_folder(folder.to_string_lossy().into_owned().into());
                    window.set_auto_stop_minutes(20);
                    window.invoke_add_app();
                    let apps = window.get_apps();
                    let mut app = apps.row_data(0).context("Add app did not create a form row")?;
                    app.name = "Test app".into();
                    app.executable = std::env::current_exe()?.to_string_lossy().into_owned().into();
                    app.arguments = "_probe-window".into();
                    app.shared = true;
                    apps.set_row_data(0, app);
                    window.set_browser_enabled(true);
                    window.set_browser_executable(r"C:\Apps\chrome.exe".into());
                    window.set_browser_urls("https://example.com/\nhttps://example.org/".into());
                    window.set_dirty(true);
                    window.invoke_save_project();
                }
                1 => {
                    let data = read()?;
                    ensure!(data.projects.len() == 1, "new project was not saved");
                    ensure!(!window.get_notice_error(), "save failed: {}", window.get_notice());
                    original_id = Some(data.projects[0].id);
                    ensure!(data.projects[0].apps.len() == 1 && data.projects[0].apps[0].shared && data.projects[0].browser.as_ref().unwrap().urls.len() == 2, "App or browser configuration did not persist");
                    window.set_project_name("Jurabox — desktop manager".into());
                    window.set_dirty(true);
                    window.invoke_save_project();
                }
                2 => {
                    let data = read()?;
                    ensure!(data.projects.len() == 1 && Some(data.projects[0].id) == original_id, "renaming changed project identity");
                    ensure!(data.projects[0].name == "Jurabox — desktop manager", "rename did not persist");
                    ensure!(data.projects[0].apps.len() == 1 && window.get_apps().row_count() == 1, "Saving unrelated fields lost the app setup");
                    window.invoke_new_project();
                    window.set_project_name("Research notes".into());
                    window.set_auto_stop_enabled(false);
                    window.set_dirty(true);
                    window.invoke_save_project();
                }
                3 => {
                    let data = read()?;
                    ensure!(data.projects.len() == 2, "second project was not saved");
                    ensure!(data.projects[1].folder.is_none() && data.projects[1].inactivity_minutes.is_none(), "folderless project or Off policy did not persist");
                    window.invoke_select_project(0);
                    window.set_project_name("Unsaved draft".into());
                    window.set_dirty(true);
                    window.invoke_select_project(1);
                    ensure!(window.get_selected_index() == 0 && window.get_dirty(), "selection discarded an unsaved edit");
                    window.invoke_discard_changes();
                    ensure!(window.get_project_name() == "Jurabox — desktop manager", "discard did not restore saved settings");
                    ensure!(!window.get_session_open(), "Selecting a saved project launched a session");
                    println!("PASS create, rename, local reload, app/link form persistence, folderless project, Off policy, unsaved-edit protection, and selection does not launch");
                }
                4 => {
                    ensure!(window.get_projects().row_count() == 2, "sidebar did not refresh");
                    snapshot(&window, &artifacts.join("projects.png"))?;
                    window.set_about_visible(true);
                }
                5 => {
                    snapshot(&window, &artifacts.join("about.png"))?;
                    window.set_about_visible(false);
                    window.window().set_size(slint::LogicalSize::new(820., 650.));
                }
                6 => {
                    snapshot(&window, &artifacts.join("compact.png"))?;
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::CloseRequested)?;
                    ensure!(!window.window().is_visible(), "close request did not hide the management window");
                    native_tray_click()?;
                }
                7 => {
                    ensure!(window.window().is_visible(), "native tray click did not restore the hidden window");
                    window.invoke_hide_window();
                    ensure!(!window.window().is_visible(), "Hide to tray failed");
                    tray.invoke_show_window();
                    ensure!(window.window().is_visible(), "tray Show action failed");
                    println!("PASS close-to-tray, native tray click, Hide to tray, and Show action");
                    window.invoke_hide_window();
                    #[cfg(windows)]
                    ensure!(crate::platform::ManagerInstance::acquire_named(&instance_name)?.is_none(), "a second instance guard was incorrectly accepted");
                }
                8 => {
                    ensure!(window.window().is_visible(), "second instance activation did not restore the manager");
                    println!("PASS instance guard and activation event");
                    window.invoke_select_project(1);
                    window.set_delete_pending(true);
                    window.invoke_delete_project();
                }
                9 => {
                    ensure!(read()?.projects.len() == 1, "project deletion was not saved");
                    ensure!(std::fs::read_to_string(folder.join("work.txt"))? == "preserve this file", "deletion changed project files");
                    // Introduce a write failure only within this check's owned temp dir.
                    std::fs::rename(&settings, &backup)?;
                    std::fs::write(&settings, "block directory creation")?;
                    window.set_project_name("Cannot save this draft".into());
                    window.set_dirty(true);
                    window.invoke_save_project();
                }
                10 => {
                    ensure!(window.get_notice_error() && window.get_dirty(), "write failure discarded the draft or was not reported");
                    ensure!(window.get_projects().row_data(0).unwrap().name == "Jurabox — desktop manager", "failed save changed the saved sidebar state");
                    std::fs::remove_file(&settings)?;
                    std::fs::rename(&backup, &settings)?;
                    ensure!(read()?.projects[0].name == "Jurabox — desktop manager", "failed save changed saved settings");
                    window.invoke_discard_changes();
                    println!("PASS deletion preserves files; failed save preserves saved data and the unsaved draft");
                    // Layout fixture only: the separate workspace exercise checks
                    // real sessions. Ensure all session controls fit the compact rail.
                    window.set_session_open(true);
                    window.set_active_workspace("Active: layout fixture".into());
                }
                11 => {
                    snapshot(&window, &artifacts.join("session-controls.png"))?;
                    window.set_session_open(false);
                    window.set_active_workspace("No project active".into());
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerScrolled { position: slint::LogicalPosition::new(600., 350.), delta_x: 0., delta_y: -240. })?;
                }
                12 => {
                    snapshot(&window, &artifacts.join("apps-scrolled.png"))?;
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerScrolled { position: slint::LogicalPosition::new(600., 350.), delta_x: 0., delta_y: -400. })?;
                }
                13 => {
                    snapshot(&window, &artifacts.join("browser-scrolled.png"))?;
                    window.invoke_add_compose();
                    let drafts = window.get_workloads();
                    let mut draft = drafts.row_data(0).context("Compose form was not added")?;
                    draft.name = "Project database".into();
                    draft.executable = std::env::current_exe()?.to_string_lossy().into_owned().into();
                    draft.compose_file = folder.join("compose.yaml").to_string_lossy().into_owned().into();
                    draft.docker_context = "desktop-linux".into();
                    draft.folder = folder.to_string_lossy().into_owned().into();
                    drafts.set_row_data(0, draft);
                    window.set_dirty(true); window.invoke_save_project();
                }
                14 => {
                    let data = read()?;
                    ensure!(data.projects[0].workloads.len() == 1 && data.projects[0].workloads[0].kind == crate::projects::WorkloadKind::DockerCompose && data.projects[0].workloads[0].compose.as_ref().is_some_and(|value| value.context == "desktop-linux"), "Compose form did not persist");
                    ensure!(!window.get_session_open(), "Saving Compose settings opened a workspace");
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerScrolled { position: slint::LogicalPosition::new(600., 350.), delta_x: 0., delta_y: -380. })?;
                }
                15 => {
                    snapshot(&window, &artifacts.join("compose-editor.png"))?;
                    window.invoke_remove_workload(0); window.invoke_save_project();
                }
                16 => {
                    ensure!(read()?.projects[0].workloads.is_empty(), "Compose form removal did not persist");
                    println!("PASS Compose form add/save/reload/remove without starting Docker");
                    window.window().set_size(slint::LogicalSize::new(1040., 740.));
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerScrolled { position: slint::LogicalPosition::new(600., 350.), delta_x: 0., delta_y: 5000. })?;
                    window.invoke_open_app_picker(false);
                }
                17 => {
                    if window.get_apps_loading() { return Ok(false); }
                    ensure!(window.get_installed_apps().iter().any(|row| row.name == "Command Prompt"), "installed-app discovery lost Command Prompt");
                    println!("Catalog: {} apps, {} Windows app IDs", window.get_installed_apps().row_count(), window.get_installed_apps().iter().filter(|row| !row.app_id.is_empty()).count());
                    snapshot(&window, &artifacts.join("app-picker.png"))?;
                    window.invoke_focus_app_search();
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::KeyPressed { text: "COMMAND PROMPT".into() })?;
                    ensure!(window.get_app_search() == "COMMAND PROMPT", "search did not receive keyboard focus");
                    ensure!(window.get_installed_apps().row_count() > 0 && window.get_installed_apps().iter().all(|row| row.name.to_lowercase().contains("command prompt")), "app search did not filter by friendly name ignoring case");
                }
                18 => {
                    snapshot(&window, &artifacts.join("app-search.png"))?;
                    let index = window.get_installed_apps().iter().position(|row| row.name == "Command Prompt").context("Command Prompt disappeared from search")?;
                    window.invoke_choose_app(index as i32);
                    let row = window.get_apps().iter().last().context("selection added no app")?;
                    ensure!(row.name == "Command Prompt" && row.new_console && !row.advanced && !row.executable.is_empty(), "app selection did not fill launch details with Advanced collapsed");
                    ensure!(!window.get_picker_visible() && window.get_dirty(), "selection did not close picker and mark the draft");
                    window.invoke_save_project();
                }
                19 => {
                    ensure!(read()?.projects[0].apps.iter().any(|app| app.name == "Command Prompt" && app.new_console), "selected app did not persist");
                    window.invoke_open_app_picker(true);
                    ensure!(window.get_installed_apps().iter().all(|row| row.name == "Google Chrome" || row.name == "Microsoft Edge"), "browser picker included non-browser names");
                    ensure!(window.get_installed_apps().row_count() > 0, "no supported installed browser found on this Windows host");
                }
                20 => {
                    snapshot(&window, &artifacts.join("browser-picker.png"))?;
                    window.invoke_choose_app(0);
                    ensure!(window.get_browser_name() != "Custom browser" && window.get_dirty(), "browser selection did not fill the friendly name");
                    window.invoke_save_project();
                }
                21 => {
                    ensure!(!window.get_dirty(), "browser selection did not save");
                    window.invoke_open_app_picker(false);
                    window.set_app_search("no-app-matches-this-jurabox-check".into()); window.invoke_search_apps();
                    ensure!(window.get_installed_apps().row_count() == 0, "empty search retained stale choices");
                }
                22 => {
                    snapshot(&window, &artifacts.join("app-picker-empty.png"))?;
                    window.set_picker_visible(false);
                    ensure!(!window.get_dirty(), "cancelling the picker changed the saved setup");
                    window.invoke_open_app_picker(false);
                    window.set_app_search("ChatGPT".into()); window.invoke_search_apps();
                    selected_windows_app = window.get_installed_apps().iter().find(|row| !row.app_id.is_empty()).map(|row| row.app_id.to_string());
                    if selected_windows_app.is_none() {
                        window.set_app_search("".into()); window.invoke_search_apps();
                        selected_windows_app = window.get_installed_apps().iter().find(|row| !row.app_id.is_empty()).map(|row| row.app_id.to_string());
                    }
                }
                23 => {
                    snapshot(&window, &artifacts.join("windows-app-search.png"))?;
                    if let Some(id) = &selected_windows_app {
                        let index = window.get_installed_apps().iter().position(|row| row.app_id == id.as_str()).context("Windows app choice disappeared")?;
                        window.invoke_choose_app(index as i32);
                        window.invoke_save_project();
                    } else { window.set_picker_visible(false); }
                }
                24 => {
                    if let Some(id) = &selected_windows_app {
                        ensure!(read()?.projects[0].apps.iter().any(|app| app.app_id.as_ref() == Some(id) && app.executable.as_os_str().is_empty()), "Windows app selection did not save its stable identity");
                    }
                    window.window().set_size(slint::LogicalSize::new(820., 650.));
                    window.invoke_open_app_picker(false);
                }
                25 => {
                    snapshot(&window, &artifacts.join("app-picker-compact.png"))?;
                    window.set_picker_visible(false);
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerScrolled { position: slint::LogicalPosition::new(600., 350.), delta_x: 0., delta_y: -220. })?;
                }
                26 => {
                    snapshot(&window, &artifacts.join("friendly-apps.png"))?;
                    println!("PASS installed-app discovery, friendly-name search, empty results, app/browser selection, Windows app IDs, collapsed Advanced, save/reload and cancel without edits");
                    window.invoke_new_project();
                    window.set_project_name("Zed project example".into());
                    window.set_project_folder(folder.to_string_lossy().into_owned().into());
                    window.set_dirty(true);
                    window.invoke_open_app_picker(false);
                    window.set_app_search("Zed".into()); window.invoke_search_apps();
                }
                27 => {
                    let index = window.get_installed_apps().iter().position(|row| crate::projects::is_zed(std::path::Path::new(row.executable.as_str()))).context("Zed was not found on this pilot host")?;
                    window.invoke_choose_app(index as i32);
                    let model = window.get_apps();
                    let mut app = model.row_data(0).context("Zed selection did not add a row")?;
                    ensure!(app.zed && !app.advanced && app.folder_mode == 0, "Zed project form was not detected or changed default launch behavior");
                    app.folder_mode = 1;
                    model.set_row_data(0, app);
                    window.invoke_save_project();
                }
                28 => {
                    ensure!(read()?.projects.last().unwrap().apps[0].open_folder == Some(crate::projects::OpenFolder::Project), "use-project-folder choice did not persist");
                    let model = window.get_apps(); let mut app = model.row_data(0).unwrap();
                    ensure!(app.zed && app.folder_mode == 1 && !app.advanced, "Zed choice did not reload with Advanced collapsed");
                    app.advanced = true; model.set_row_data(0, app);
                    window.window().set_size(slint::LogicalSize::new(1040., 740.));
                    window.window().dispatch_event_with_result(slint::platform::WindowEvent::PointerScrolled { position: slint::LogicalPosition::new(600., 350.), delta_x: 0., delta_y: -180. })?;
                }
                29 => {
                    snapshot(&window, &artifacts.join("zed-project-folder.png"))?;
                    let custom = folder.join("Another code project");
                    std::fs::create_dir(&custom)?;
                    let model = window.get_apps(); let mut app = model.row_data(0).unwrap();
                    app.folder_mode = 2; app.editor_folder = custom.to_string_lossy().into_owned().into();
                    model.set_row_data(0, app); window.set_dirty(true); window.invoke_save_project();
                }
                30 => {
                    ensure!(read()?.projects.last().unwrap().apps[0].open_folder == Some(crate::projects::OpenFolder::Custom(folder.join("Another code project"))), "custom folder did not persist");
                    let model = window.get_apps(); let mut app = model.row_data(0).unwrap();
                    ensure!(app.folder_mode == 2 && app.editor_folder.contains("Another code project"), "custom folder did not reload");
                    app.advanced = true; model.set_row_data(0, app);
                    window.window().set_size(slint::LogicalSize::new(820., 650.));
                }
                31 => {
                    snapshot(&window, &artifacts.join("zed-custom-folder-compact.png"))?;
                    let model = window.get_apps(); let mut app = model.row_data(0).unwrap();
                    app.folder_mode = 1; model.set_row_data(0, app);
                    window.set_project_folder("".into()); window.set_dirty(true); window.invoke_save_project();
                    ensure!(window.get_notice_error() && window.get_dirty(), "missing project folder was silently accepted");
                    window.invoke_discard_changes();
                    ensure!(window.get_apps().row_data(0).unwrap().folder_mode == 2, "discard lost the saved custom folder");
                    let model = window.get_apps(); let mut app = model.row_data(0).unwrap();
                    app.folder_mode = 0; model.set_row_data(0, app);
                    window.set_dirty(true); window.invoke_save_project();
                }
                32 => {
                    ensure!(read()?.projects.last().unwrap().apps[0].open_folder.is_none(), "Open Zed normally did not clear folder launch selection");
                    println!("PASS Zed detection, Advanced-only project selection, default unchanged, project/custom folder save/reload, missing-folder validation, discard and normal-launch opt-out");
                    window.invoke_quit(); return Ok(true);
                }
                _ => bail!("unexpected UI check step"),
            }
            step += 1;
            Ok(false)
        };
        match run_step() {
            Ok(true) => { *outcome.borrow_mut() = Some(Ok(())); }
            Ok(false) => {}
            Err(error) => {
                *outcome.borrow_mut() = Some(Err(error));
                let _ = slint::quit_event_loop();
            }
        }
    });
    slint::run_event_loop_until_quit()?;
    timer.stop();
    result
        .borrow_mut()
        .take()
        .context("UI check ended before completion")??;
    println!("UI check passed. Snapshots: target/ui-check/");
    Ok(())
}
