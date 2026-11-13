use std::sync::OnceLock;

use anyhow::Context;
use gpui_util::ResultExt;
use windows::{
    UI::{
        Color,
        ViewManagement::{UIColorType, UISettings},
    },
    Win32::{
        Foundation::*, Graphics::Dwm::*, System::LibraryLoader::LoadLibraryA,
        UI::WindowsAndMessaging::*,
    },
    core::{BOOL, PCSTR},
};

use crate::*;
use gpui::*;

pub(crate) trait HiLoWord {
    fn hiword(&self) -> u16;
    fn loword(&self) -> u16;
    fn signed_hiword(&self) -> i16;
    fn signed_loword(&self) -> i16;
}

impl HiLoWord for WPARAM {
    fn hiword(&self) -> u16 {
        ((self.0 >> 16) & 0xFFFF) as u16
    }

    fn loword(&self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }

    fn signed_hiword(&self) -> i16 {
        ((self.0 >> 16) & 0xFFFF) as i16
    }

    fn signed_loword(&self) -> i16 {
        (self.0 & 0xFFFF) as i16
    }
}

impl HiLoWord for LPARAM {
    fn hiword(&self) -> u16 {
        ((self.0 >> 16) & 0xFFFF) as u16
    }

    fn loword(&self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }

    fn signed_hiword(&self) -> i16 {
        ((self.0 >> 16) & 0xFFFF) as i16
    }

    fn signed_loword(&self) -> i16 {
        (self.0 & 0xFFFF) as i16
    }
}

pub(crate) unsafe fn get_window_long(hwnd: HWND, nindex: WINDOW_LONG_PTR_INDEX) -> isize {
    #[cfg(target_pointer_width = "64")]
    unsafe {
        GetWindowLongPtrW(hwnd, nindex)
    }
    #[cfg(target_pointer_width = "32")]
    unsafe {
        GetWindowLongW(hwnd, nindex) as isize
    }
}

pub(crate) unsafe fn set_window_long(
    hwnd: HWND,
    nindex: WINDOW_LONG_PTR_INDEX,
    dwnewlong: isize,
) -> isize {
    #[cfg(target_pointer_width = "64")]
    unsafe {
        SetWindowLongPtrW(hwnd, nindex, dwnewlong)
    }
    #[cfg(target_pointer_width = "32")]
    unsafe {
        SetWindowLongW(hwnd, nindex, dwnewlong as i32) as isize
    }
}

pub(crate) fn windows_credentials_target_name(url: &str) -> String {
    format!("zed:url={}", url)
}

pub(crate) fn load_cursor(style: CursorStyle) -> Option<HCURSOR> {
    static ARROW: OnceLock<SafeCursor> = OnceLock::new();
    static IBEAM: OnceLock<SafeCursor> = OnceLock::new();
    static CROSS: OnceLock<SafeCursor> = OnceLock::new();
    static HAND: OnceLock<SafeCursor> = OnceLock::new();
    static SIZEWE: OnceLock<SafeCursor> = OnceLock::new();
    static SIZENS: OnceLock<SafeCursor> = OnceLock::new();
    static SIZENWSE: OnceLock<SafeCursor> = OnceLock::new();
    static SIZENESW: OnceLock<SafeCursor> = OnceLock::new();
    static NO: OnceLock<SafeCursor> = OnceLock::new();
    let (lock, name) = match style {
        CursorStyle::IBeam | CursorStyle::IBeamCursorForVerticalLayout => (&IBEAM, IDC_IBEAM),
        CursorStyle::Crosshair => (&CROSS, IDC_CROSS),
        CursorStyle::PointingHand | CursorStyle::DragLink => (&HAND, IDC_HAND),
        CursorStyle::ResizeLeft
        | CursorStyle::ResizeRight
        | CursorStyle::ResizeLeftRight
        | CursorStyle::ResizeColumn => (&SIZEWE, IDC_SIZEWE),
        CursorStyle::ResizeUp
        | CursorStyle::ResizeDown
        | CursorStyle::ResizeUpDown
        | CursorStyle::ResizeRow => (&SIZENS, IDC_SIZENS),
        CursorStyle::ResizeUpLeftDownRight => (&SIZENWSE, IDC_SIZENWSE),
        CursorStyle::ResizeUpRightDownLeft => (&SIZENESW, IDC_SIZENESW),
        CursorStyle::OperationNotAllowed => (&NO, IDC_NO),
        _ => (&ARROW, IDC_ARROW),
    };
    Some(
        *(*lock.get_or_init(|| {
            HCURSOR(
                unsafe { LoadImageW(None, name, IMAGE_CURSOR, 0, 0, LR_DEFAULTSIZE | LR_SHARED) }
                    .log_err()
                    .unwrap_or_default()
                    .0,
            )
            .into()
        })),
    )
}

/// This function is used to configure the dark mode for the window built-in title bar.
pub(crate) fn configure_dwm_dark_mode(hwnd: HWND, appearance: WindowAppearance) {
    let dark_mode_enabled: BOOL = match appearance {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => true.into(),
        WindowAppearance::Light | WindowAppearance::VibrantLight => false.into(),
    };
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode_enabled as *const _ as _,
            std::mem::size_of::<BOOL>() as u32,
        )
        .log_err();
    }
}

#[inline]
pub(crate) fn logical_point(x: f32, y: f32, scale_factor: f32) -> Point<Pixels> {
    Point {
        x: px(x / scale_factor),
        y: px(y / scale_factor),
    }
}

// https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/apply-windows-themes
#[inline]
pub(crate) fn system_appearance() -> Result<WindowAppearance> {
    let ui_settings = UISettings::new()?;
    let foreground_color = ui_settings.GetColorValue(UIColorType::Foreground)?;
    // If the foreground is light, then is_color_light will evaluate to true,
    // meaning Dark mode is enabled.
    if is_color_light(&foreground_color) {
        Ok(WindowAppearance::Dark)
    } else {
        Ok(WindowAppearance::Light)
    }
}

#[inline(always)]
fn is_color_light(color: &Color) -> bool {
    ((5 * color.G as u32) + (2 * color.R as u32) + color.B as u32) > (8 * 128)
}

pub(crate) fn with_dll_library<R, F>(dll_name: PCSTR, f: F) -> Result<R>
where
    F: FnOnce(HMODULE) -> Result<R>,
{
    let library = unsafe {
        LoadLibraryA(dll_name).with_context(|| format!("Loading dll: {}", dll_name.display()))?
    };
    let result = f(library);
    unsafe {
        FreeLibrary(library)
            .with_context(|| format!("Freeing dll: {}", dll_name.display()))
            .log_err();
    }
    result
}

/// Pets patch: Win32 failure codes meaning "this window handle no longer exists".
fn is_window_gone_error(code: windows::core::HRESULT) -> bool {
    code == ERROR_INVALID_WINDOW_HANDLE.to_hresult() || code == DRAGDROP_E_INVALIDHWND
}

/// Pets patch: whether a failed call on `hwnd` is the expected race with its destruction.
///
/// Messages and deferred calls (vsync redraw, posted tasks, `Drop` of the window) can reach a
/// window or the platform window after it was destroyed: the user closed it, or the app is
/// shutting down. Such a failure is not an error — only when the code says "invalid window
/// handle" *and* the window is really gone. Any other failure, or this code on a live window,
/// stays an error.
pub(crate) fn is_expected_after_destroy(code: windows::core::HRESULT, hwnd: HWND) -> bool {
    is_window_gone_error(code) && !unsafe { IsWindow(Some(hwnd)) }.as_bool()
}

/// Pets patch: like [`ResultExt::log_err`] for a Win32 call on `hwnd`, but a failure because
/// the window was already destroyed goes to DEBUG (`crates/pets-shell/LOGGING.md`: nothing for
/// a person to do), not to ERROR and the system log.
#[track_caller]
pub(crate) fn log_hwnd_err<T>(result: windows::core::Result<T>, hwnd: HWND) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) if is_expected_after_destroy(error.code(), hwnd) => {
            log::debug!("window {:?} already destroyed: {error}", hwnd.0);
            None
        }
        Err(error) => Err::<T, _>(error).log_err(),
    }
}

#[cfg(test)]
mod pets_destroyed_window_tests {
    // Not `super::*`: it brings `gpui::test`, which shadows `#[test]`.
    use super::{is_expected_after_destroy, log_hwnd_err};
    use windows::Win32::{Foundation::*, UI::WindowsAndMessaging::*};
    use windows::core::w;

    /// A real top-level window, destroyed right away: its handle is what late calls see.
    fn destroyed_hwnd() -> HWND {
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("pets destroyed window test"),
                WS_OVERLAPPED,
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
            .expect("create test window");
            DestroyWindow(hwnd).expect("destroy test window");
            hwnd
        }
    }

    #[test]
    fn pets_late_calls_on_destroyed_window_are_expected() {
        let hwnd = destroyed_hwnd();
        let post = unsafe { PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)) };
        let error = post.clone().expect_err("posting to a destroyed window fails");
        assert!(is_expected_after_destroy(error.code(), hwnd));
        let destroy = unsafe { DestroyWindow(hwnd) }.expect_err("second destroy fails");
        assert!(is_expected_after_destroy(destroy.code(), hwnd));
        assert!(is_expected_after_destroy(DRAGDROP_E_INVALIDHWND, hwnd));
        assert_eq!(log_hwnd_err(post, hwnd), None);
    }

    #[test]
    fn pets_invalid_handle_on_live_window_stays_error() {
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("pets live window test"),
                WS_OVERLAPPED,
                0,
                0,
                10,
                10,
                None,
                None,
                None,
                None,
            )
            .expect("create test window");
            let code = ERROR_INVALID_WINDOW_HANDLE.to_hresult();
            assert!(!is_expected_after_destroy(code, hwnd));
            DestroyWindow(hwnd).expect("destroy test window");
        }
        // Another failure on a destroyed window is still an error.
        assert!(!is_expected_after_destroy(E_FAIL, destroyed_hwnd()));
    }
}
