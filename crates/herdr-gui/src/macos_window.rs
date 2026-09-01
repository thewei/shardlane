#[cfg(target_os = "macos")]
use gpui::Window;

#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSFloatingWindowLevel, NSNormalWindowLevel, NSView,
};
#[cfg(target_os = "macos")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[cfg(target_os = "macos")]
pub fn apply(
    window: &Window,
    opacity: f32,
    always_on_top: bool,
    forced_dark: Option<bool>,
) -> Result<(), String> {
    let handle = <Window as HasWindowHandle>::window_handle(window)
        .map_err(|error| format!("window handle unavailable: {error}"))?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err("Shardlane expected an AppKit window handle on macOS".to_string());
    };
    let view = unsafe { &*(handle.ns_view.as_ptr().cast::<NSView>()) };
    let native_window = view
        .window()
        .ok_or_else(|| "AppKit view is not attached to an NSWindow".to_string())?;
    native_window.setAlphaValue(f64::from(opacity.clamp(0.55, 1.0)));
    let appearance = forced_dark.and_then(|dark| {
        let name = unsafe {
            if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        NSAppearance::appearanceNamed(name)
    });
    native_window.setAppearance(appearance.as_deref());
    native_window.setLevel(if always_on_top {
        NSFloatingWindowLevel
    } else {
        NSNormalWindowLevel
    });
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn apply(
    _window: &gpui::Window,
    _opacity: f32,
    _always_on_top: bool,
    _forced_dark: Option<bool>,
) -> Result<(), String> {
    Ok(())
}
