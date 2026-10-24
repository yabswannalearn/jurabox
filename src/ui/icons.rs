//! UI-owned images, populated by one deduplicated background Shell worker.
use super::*;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::mpsc,
};

#[derive(Default)]
struct Cache {
    images: HashMap<String, slint::Image>,
    pending: HashSet<String>,
    sender: Option<mpsc::Sender<String>>,
}
thread_local! { static CACHE: RefCell<Cache> = RefCell::new(Cache::default()); }

pub(super) fn key(executable: &str, app_id: &str) -> String {
    if !app_id.is_empty() {
        format!("app:{app_id}")
    } else {
        format!(
            "exe:{}",
            executable
                .trim()
                .trim_matches('"')
                .replace('/', "\\")
                .to_lowercase()
        )
    }
}

pub(super) fn cached(target: &str) -> slint::Image {
    CACHE.with_borrow(|cache| cache.images.get(target).cloned().unwrap_or_default())
}

pub(super) fn get(target: &str) -> slint::Image {
    CACHE.with_borrow_mut(|cache| {
        if let Some(image) = cache.images.get(target) {
            return image.clone();
        }
        if target != "exe:"
            && cache.pending.insert(target.to_owned())
            && let Some(sender) = &cache.sender
        {
            let _ = sender.send(target.to_owned());
        }
        slint::Image::default()
    })
}

#[cfg(windows)]
pub(super) fn setup(window: &AppWindow) {
    window.on_resolve_app_icon(|target| get(&target));
    let (sender, receiver) = mpsc::channel::<String>();
    CACHE.with_borrow_mut(|cache| {
        cache.sender = Some(sender);
        cache.pending.clear();
    });
    let weak = window.as_weak();
    std::thread::spawn(move || {
        while let Ok(target) = receiver.recv() {
            let pixels = match crate::platform::apps::icon(&target) {
                Ok(pixels) => Some(pixels),
                Err(error) => {
                    eprintln!("App icon unavailable for {target}: {error:#}");
                    None
                }
            };
            if weak
                .upgrade_in_event_loop(move |window| loaded(&window, target, pixels))
                .is_err()
            {
                break;
            }
        }
    });
}

#[cfg(windows)]
fn loaded(window: &AppWindow, target: String, pixels: Option<crate::platform::apps::IconPixels>) {
    let image = pixels
        .map(|pixels| {
            let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                &pixels.rgba,
                pixels.width,
                pixels.height,
            );
            slint::Image::from_rgba8_premultiplied(buffer)
        })
        .unwrap_or_default();
    CACHE.with_borrow_mut(|cache| {
        cache.pending.remove(&target);
        cache.images.insert(target.clone(), image.clone());
    });
    // Update only rows using this icon. Search and scrolling never extract icons.
    use slint::Model;
    macro_rules! update {
        ($model:expr) => {{
            let model = $model;
            for index in 0..model.row_count() {
                if let Some(mut row) = model.row_data(index)
                    && row.icon_key == target
                {
                    row.icon = image.clone();
                    model.set_row_data(index, row);
                }
            }
        }};
    }
    update!(window.get_apps());
    update!(window.get_installed_apps());
    update!(window.get_session_apps());
    if key(&window.get_browser_executable(), "") == target {
        window.set_browser_icon(image);
    }
}
