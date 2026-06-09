//! Runtime, theme-aware window icon. On Windows the installed `.exe` icon is
//! static, but the live WINDOW icon (what the taskbar shows for a running window)
//! can be swapped via Tauri's `set_icon`. The frontend calls `set_window_icon`
//! whenever the app theme toggles, passing `dark`. Both rasters are baked into the
//! binary (`include_bytes!`) so there is no runtime asset path to resolve. The
//! tiles mirror Orrery Logo Lab v2's "App icon": dark = violet→teal (`ai-grad`,
//! the bundled icon), light = `#fff`→`#e9ecf3` (`ai-light`, app-icon-light.svg).

use tauri::image::Image;

/// Dark-theme tile — the bundled app-icon raster (256px).
const ICON_DARK: &[u8] = include_bytes!("../icons/128x128@2x.png");
/// Light-theme tile — generated from app-icon-light.svg (256px).
const ICON_LIGHT: &[u8] = include_bytes!("../icons/icon-light.png");

/// Set the live window icon to match the app theme. Decoding needs the tauri
/// `image-png` feature (see Cargo.toml). Best-effort from the caller's side.
#[tauri::command(async)]
pub fn set_window_icon(window: tauri::WebviewWindow, dark: bool) -> Result<(), String> {
    crate::perf::timed("set_window_icon", || {
        let img = Image::from_bytes(if dark { ICON_DARK } else { ICON_LIGHT })
            .map_err(|e| e.to_string())?;
        window.set_icon(img).map_err(|e| e.to_string())
    })
}
