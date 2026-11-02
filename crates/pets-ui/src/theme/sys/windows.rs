//! Windows: тема и акцент — реестр HKCU, контрастность — `SPI_GETHIGHCONTRAST` и `GetSysColor`, политика —
//! `Software\Policies\Pets\<App>` в HKLM и HKCU; изменения — `RegNotifyChangeKeyValue` и
//! `FindFirstChangeNotificationW` в одном потоке без опроса.
//!
//! Зачем свой поток при `observe_window_appearance` gpui: окно gpui сообщает только смену светлая ↔ тёмная
//! (`ImmersiveColorSet` с другим видом), а смена акцента и контрастности его не меняет.

use std::path::{Path, PathBuf};
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::Graphics::Gdi::{
    COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_HOTLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT, GetSysColor,
    SYS_COLOR_INDEX,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FindFirstChangeNotificationW,
    FindNextChangeNotification,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_BINARY,
    RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW,
};
use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForMultipleObjects};
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW};

use super::Signal;
use crate::theme::{AccentPalette, Color, HcColors, OsLook, POLICY_KEY, Policy};

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const ACCENT: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
const DWM: &str = r"Software\Microsoft\Windows\DWM";
const HIGH_CONTRAST: &str = r"Control Panel\Accessibility\HighContrast";

/// «Параметры» пишут несколько значений подряд: пауза перед чтением.
const SETTLE_REGISTRY: Duration = Duration::from_millis(50);
/// Редакторы сохраняют файл в несколько записей.
const SETTLE_FILES: Duration = Duration::from_millis(150);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Значение реестра; `None` — нет ключа, значения или другой тип.
fn reg_raw(root: HKEY, key: &str, value: &str, kind: u32) -> Option<Vec<u8>> {
    let (key, value) = (wide(key), wide(value));
    let mut len = 0u32;
    // SAFETY: строки с NUL живут до конца вызова; запрос размера без буфера.
    let rc = unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            value.as_ptr(),
            kind,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut len,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    // SAFETY: буфер `len` байт; функция пишет не больше `len` и обновляет его.
    let rc = unsafe {
        RegGetValueW(
            root,
            key.as_ptr(),
            value.as_ptr(),
            kind,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &raw mut len,
        )
    };
    (rc == ERROR_SUCCESS).then(|| {
        buf.truncate(len as usize);
        buf
    })
}

fn reg_dword(root: HKEY, key: &str, value: &str) -> Option<u32> {
    let b = reg_raw(root, key, value, RRF_RT_REG_DWORD)?;
    Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))
}

fn reg_string(root: HKEY, key: &str, value: &str) -> Option<String> {
    let b = reg_raw(root, key, value, RRF_RT_REG_SZ)?;
    let units: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|u| *u != 0)
        .collect();
    String::from_utf16(&units).ok()
}

pub(crate) fn os_look() -> OsLook {
    let dark = reg_dword(HKEY_CURRENT_USER, PERSONALIZE, "AppsUseLightTheme") == Some(0);
    let accent = reg_raw(HKEY_CURRENT_USER, ACCENT, "AccentPalette", RRF_RT_REG_BINARY)
        .and_then(|b| AccentPalette::from_registry(&b))
        .or_else(|| reg_dword(HKEY_CURRENT_USER, DWM, "AccentColor").map(AccentPalette::from_abgr))
        .unwrap_or_default();
    let high_contrast = high_contrast();
    OsLook {
        dark,
        high_contrast,
        accent,
        hc: high_contrast.then(hc_colors),
    }
}

fn hc_colors() -> HcColors {
    let c = |index: SYS_COLOR_INDEX| {
        // SAFETY: без указателей; индекс из констант COLOR_*.
        let v = unsafe { GetSysColor(index) };
        // COLORREF — 0x00BBGGRR.
        let [r, g, b, _] = v.to_le_bytes();
        Color { r, g, b, a: 255 }
    };
    HcColors {
        window: c(COLOR_WINDOW),
        window_text: c(COLOR_WINDOWTEXT),
        highlight: c(COLOR_HIGHLIGHT),
        highlight_text: c(COLOR_HIGHLIGHTTEXT),
        gray_text: c(COLOR_GRAYTEXT),
        hotlight: c(COLOR_HOTLIGHT),
    }
}

fn high_contrast() -> bool {
    let mut hc = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: структура нужного размера с заполненным `cbSize`; функция только пишет в неё.
    let ok = unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cbSize, (&raw mut hc).cast(), 0) };
    ok != 0 && hc.dwFlags & HCF_HIGHCONTRASTON != 0
}

pub(crate) fn policy(app: &str) -> Policy {
    let key = format!(r"{POLICY_KEY}\{app}");
    let read = |root: HKEY| Policy {
        theme: reg_string(root, &key, "Theme")
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty()),
        allow_change: reg_dword(root, &key, "AllowThemeChange").map(|v| v != 0),
    };
    let (machine, user) = (read(HKEY_LOCAL_MACHINE), read(HKEY_CURRENT_USER));
    Policy {
        theme: machine.theme.or(user.theme),
        allow_change: machine.allow_change.or(user.allow_change),
    }
}

/// Ключ HKCU с событием изменения. Живёт до выхода процесса (поток не завершается раньше).
struct Watched {
    key: HKEY,
    event: HANDLE,
}

impl Watched {
    fn open(path: &str) -> Option<Self> {
        let path = wide(path);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: путь с NUL; `key` — выходной параметр.
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_NOTIFY, &raw mut key) };
        if rc != ERROR_SUCCESS {
            return None;
        }
        // SAFETY: безымянное событие с автосбросом, без атрибутов безопасности.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if event.is_null() {
            return None;
        }
        let w = Self { key, event };
        w.arm().then_some(w)
    }

    /// Подписка на одно следующее изменение значений ключа.
    fn arm(&self) -> bool {
        // SAFETY: ключ открыт с KEY_NOTIFY, событие живо; асинхронный режим.
        let rc = unsafe { RegNotifyChangeKeyValue(self.key, 0, REG_NOTIFY_CHANGE_LAST_SET, self.event, 1) };
        rc == ERROR_SUCCESS
    }
}

/// Уведомление об изменениях файлов папки (без подпапок); `None` — папки нет.
fn watch_dir(dir: &Path) -> Option<HANDLE> {
    let path: Vec<u16> = dir.as_os_str().encode_wide_nul();
    // SAFETY: путь с NUL живёт до конца вызова.
    let h = unsafe {
        FindFirstChangeNotificationW(
            path.as_ptr(),
            0,
            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE,
        )
    };
    (h != INVALID_HANDLE_VALUE && !h.is_null()).then_some(h)
}

trait EncodeWideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}

impl EncodeWideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt as _;
        self.encode_wide().chain(std::iter::once(0)).collect()
    }
}

/// Поток `pets-ui-theme`: ждёт изменений вида ОС и папки тем; `on` → `false` — выход.
pub(crate) fn watch(dir: Option<PathBuf>, mut on: Box<dyn FnMut(Signal) -> bool + Send>) {
    let spawned = std::thread::Builder::new().name("pets-ui-theme".into()).spawn(move || {
        let keys: Vec<Watched> = [PERSONALIZE, ACCENT, DWM, HIGH_CONTRAST]
            .iter()
            .filter_map(|k| Watched::open(k))
            .collect();
        let dir_handle = dir.as_deref().and_then(watch_dir);
        if dir.is_some() && dir_handle.is_none() {
            tracing::info!(target: "pets_ui::theme", ?dir, "theme folder is not watched (missing?)");
        }
        let mut handles: Vec<HANDLE> = keys.iter().map(|w| w.event).collect();
        handles.extend(dir_handle);
        loop {
            let Ok(count) = u32::try_from(handles.len()) else {
                return;
            };
            if count == 0 {
                return;
            }
            // SAFETY: массив живых описателей; ждём любой без таймаута.
            let rc = unsafe { WaitForMultipleObjects(count, handles.as_ptr(), 0, INFINITE) };
            let Some(i) = rc
                .checked_sub(WAIT_OBJECT_0)
                .map(|i| i as usize)
                .filter(|i| *i < handles.len())
            else {
                tracing::warn!(target: "pets_ui::theme", rc, "theme watch: wait failed");
                return;
            };
            let signal = if let Some(w) = keys.get(i) {
                std::thread::sleep(SETTLE_REGISTRY);
                w.arm();
                Signal::Look
            } else {
                std::thread::sleep(SETTLE_FILES);
                // SAFETY: описатель от FindFirstChangeNotificationW, жив.
                if unsafe { FindNextChangeNotification(handles[i]) } == 0 {
                    tracing::warn!(target: "pets_ui::theme", "theme folder watch stopped");
                    handles.pop();
                }
                Signal::Dir
            };
            if !on(signal) {
                return;
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(target: "pets_ui::theme", error = %e, "theme watch thread");
    }
}
