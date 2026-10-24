//! Read authentic Windows app icons on a worker, without launching the app.
use super::catalog::Apartment;
use anyhow::{Result, ensure};
use windows::{
    Win32::{
        Foundation::SIZE,
        Graphics::Gdi::{
            BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS,
            DeleteDC, DeleteObject, GetDIBits, GetObjectW, HBITMAP,
        },
        UI::Shell::{
            FOLDERID_AppsFolder, IShellItemImageFactory, KF_FLAG_DEFAULT,
            SHCreateItemFromParsingName, SHCreateItemInKnownFolder, SIIGBF_ICONONLY,
        },
    },
    core::HSTRING,
};

pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn icon(target: &str) -> Result<Pixels> {
    let _com = Apartment::new()?;
    // SAFETY: Shell metadata is read on the icon worker. The bitmap and DC are
    // owned by this call and released on every return path.
    unsafe {
        let item: IShellItemImageFactory = if let Some(id) = target.strip_prefix("app:") {
            SHCreateItemInKnownFolder(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, &HSTRING::from(id))?
        } else {
            SHCreateItemFromParsingName(&HSTRING::from(target.trim_start_matches("exe:")), None)?
        };
        let bitmap = item.GetImage(SIZE { cx: 48, cy: 48 }, SIIGBF_ICONONLY)?;
        let result = bitmap_pixels(bitmap);
        let _ = DeleteObject(bitmap);
        result
    }
}

unsafe fn bitmap_pixels(bitmap: HBITMAP) -> Result<Pixels> {
    let mut details = BITMAP::default();
    // SAFETY: Buffer sizes match their native structures; requested dimensions
    // are bounded before allocation. A negative DIB height gives top-down rows.
    unsafe {
        ensure!(
            GetObjectW(
                bitmap,
                std::mem::size_of::<BITMAP>() as i32,
                Some((&mut details as *mut BITMAP).cast())
            ) != 0,
            "Cannot inspect app icon"
        );
        let width = details.bmWidth;
        let height = details.bmHeight.abs();
        ensure!(
            width > 0 && width <= 256 && height > 0 && height <= 256,
            "Unexpected app icon size"
        );
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = CreateCompatibleDC(None);
        ensure!(!dc.is_invalid(), "Cannot read app icon");
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        let read = GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            Some(rgba.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        ensure!(read == height, "Cannot read app icon pixels");
        let opaque = rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0);
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            if opaque {
                pixel[3] = 255;
            }
        }
        // Some packaged apps publish a white mark on transparency. Give those
        // marks a neutral tile so they remain visible on the light app surface.
        let visible: Vec<_> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 32)
            .collect();
        if !visible.is_empty()
            && visible.iter().all(|p| {
                p[..3]
                    .iter()
                    .all(|c| u16::from(*c) * 255 >= u16::from(p[3]) * 224)
            })
        {
            for pixel in rgba.as_chunks_mut::<4>().0 {
                let transparent = u16::from(255 - pixel[3]);
                for (channel, background) in pixel[..3].iter_mut().zip([52u16, 69, 82]) {
                    *channel =
                        (u16::from(*channel) + background * transparent / 255).min(255) as u8;
                }
                pixel[3] = 255;
            }
        }
        Ok(Pixels {
            width: width as u32,
            height: height as u32,
            rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_a_packaged_app_icon_when_one_is_installed() {
        let apps = super::super::installed_apps();
        let selected = apps
            .iter()
            .find(|app| app.name == "ChatGPT" && app.app_id.is_some())
            .or_else(|| apps.iter().find(|app| app.app_id.is_some()));
        if let Some(app) = selected {
            let pixels = icon(&format!("app:{}", app.app_id.as_ref().unwrap())).unwrap();
            assert!(pixels.width > 0 && pixels.height > 0);
            assert!(
                pixels
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] > 0)
            );
        }
    }
    #[test]
    fn reads_a_real_windows_icon_without_launching_the_app() {
        let system = std::env::var("SystemRoot").unwrap();
        let icon = icon(&format!("exe:{system}\\System32\\cmd.exe")).unwrap();
        assert_eq!(
            icon.rgba.len(),
            icon.width as usize * icon.height as usize * 4
        );
        assert!(
            icon.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0)
        );
        assert!(icon.width <= 256 && icon.height <= 256);
    }
}
