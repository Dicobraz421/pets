//! macOS (Postman MAC-04): светлая/тёмная — `AppleInterfaceStyle` глобального домена (то, что `NSAppearance`
//! приложения берёт у системы; «Автоматически» система переключает сама), акцент — `NSColor.controlAccentColor` в
//! sRGB, политика — принудительные значения профиля конфигурации (`CFPreferencesAppValueIsForced`, домен — пакет
//! приложения), изменения — распределённые уведомления `AppleInterfaceThemeChangedNotification` и
//! `AppleColorPreferencesChangedNotification` в своём потоке с `CFRunLoop`, без опроса.
//!
//! Высокой контрастности Windows на macOS нет («Увеличить контраст» цвета не заменяет): `high_contrast` — `false`.
//! Папку тем пользователя на macOS не отслеживаем: новые файлы — со следующего запуска.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::time::Duration;

use objc2_app_kit::{NSColor, NSColorSpace};
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFNotificationCenter, CFNotificationName, CFNotificationSuspensionBehavior, CFNumber,
    CFPreferencesAppSynchronize, CFPreferencesAppValueIsForced, CFPreferencesCopyAppValue, CFRetained, CFRunLoop,
    CFString, CFType, kCFPreferencesAnyApplication, kCFPreferencesCurrentApplication,
};

use super::Signal;
use crate::theme::{AccentPalette, Color, OsLook, Policy};

/// Уведомления системы о смене вида: светлая/тёмная и акцент (цвет выделения).
const NOTIFICATIONS: [&str; 2] = [
    "AppleInterfaceThemeChangedNotification",
    "AppleColorPreferencesChangedNotification",
];

/// Уведомление приходит раньше, чем `cfprefsd` отдаёт новое значение: пауза перед чтением.
const SETTLE: Duration = Duration::from_millis(50);

pub(crate) fn os_look() -> OsLook {
    // SAFETY: неизменяемая константа CoreFoundation, живёт весь процесс.
    let global = unsafe { kCFPreferencesAnyApplication };
    CFPreferencesAppSynchronize(global);
    let style = CFPreferencesCopyAppValue(&CFString::from_static_str("AppleInterfaceStyle"), global);
    let dark = style
        .as_deref()
        .and_then(|v| v.downcast_ref::<CFString>())
        .is_some_and(|s| s.to_string().eq_ignore_ascii_case("dark"));
    OsLook {
        dark,
        high_contrast: false,
        accent: accent().map(AccentPalette::from_base).unwrap_or_default(),
        hc: None,
    }
}

/// Акцент системы в sRGB; цвет без компонент RGB — `None`.
fn accent() -> Option<Color> {
    let srgb = NSColorSpace::sRGBColorSpace();
    let c = NSColor::controlAccentColor().colorUsingColorSpace(&srgb)?;
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(Color {
        r: byte(c.redComponent()),
        g: byte(c.greenComponent()),
        b: byte(c.blueComponent()),
        a: 255,
    })
}

/// Принудительное значение профиля конфигурации для пакета приложения.
fn forced(key: &str) -> Option<CFRetained<CFType>> {
    // SAFETY: неизменяемая константа CoreFoundation, живёт весь процесс.
    let app = unsafe { kCFPreferencesCurrentApplication };
    let key = CFString::from_str(key);
    if !CFPreferencesAppValueIsForced(&key, app) {
        return None;
    }
    CFPreferencesCopyAppValue(&key, app)
}

/// Политика — ключи `Theme` (строка) и `AllowThemeChange` (логическое или число) профиля для пакета приложения;
/// `_app` на macOS не нужен: домен — идентификатор пакета.
pub(crate) fn policy(_app: &str) -> Policy {
    let theme = forced("Theme")
        .and_then(|v| v.downcast_ref::<CFString>().map(|s| s.to_string()))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let allow_change = forced("AllowThemeChange").and_then(|v| {
        v.downcast_ref::<CFBoolean>()
            .map(CFBoolean::as_bool)
            .or_else(|| v.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64).map(|n| n != 0))
    });
    Policy { theme, allow_change }
}

/// Состояние потока слежения: обработчик.
struct Watch {
    on: Box<dyn FnMut(Signal) -> bool + Send>,
}

/// Обработчик распределённого уведомления: `observer` — `Watch` потока слежения.
unsafe extern "C-unwind" fn changed(
    _center: *mut CFNotificationCenter,
    observer: *mut c_void,
    _name: *const CFNotificationName,
    _object: *const c_void,
    _info: *const CFDictionary,
) {
    // SAFETY: `observer` — `Watch` из `watch`, живёт до конца процесса; уведомления приходят только в его поток.
    let Some(watch) = (unsafe { observer.cast::<Watch>().as_mut() }) else {
        return;
    };
    std::thread::sleep(SETTLE);
    if !(watch.on)(Signal::Look)
        && let Some(rl) = CFRunLoop::current()
    {
        rl.stop();
    }
}

pub(crate) fn watch(_dir: Option<PathBuf>, on: Box<dyn FnMut(Signal) -> bool + Send>) {
    let spawned = std::thread::Builder::new().name("pets-ui-theme-watch".into()).spawn(move || {
        let Some(center) = CFNotificationCenter::distributed_center() else {
            tracing::warn!(target: "pets_ui::theme", "no distributed notification center: system look is not watched");
            return;
        };
        // Живёт до конца процесса: центр держит указатель, пока наблюдатель не снят.
        let watch = NonNull::from(Box::leak(Box::new(Watch { on })));
        for name in NOTIFICATIONS {
            let name = CFString::from_static_str(name);
            // SAFETY: наблюдатель — живой `Watch`, обработчик с подписью `CFNotificationCallback`; уведомления
            // доставляются в цикл этого потока.
            unsafe {
                center.add_observer(
                    watch.as_ptr().cast(),
                    Some(changed),
                    Some(&name),
                    std::ptr::null(),
                    CFNotificationSuspensionBehavior::DeliverImmediately,
                );
            }
        }
        CFRunLoop::run();
    });
    if let Err(e) = spawned {
        tracing::warn!(target: "pets_ui::theme", "theme watch thread: {e}");
    }
}
