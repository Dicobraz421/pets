//! Управляемые настройки macOS — аналог политик реестра Windows (`Software\Policies\…`): значения, которые
//! администратор задал профилем конфигурации (MDM, `/Library/Managed Preferences/<id пакета>.plist`). Домен — пакет
//! приложения (`kCFPreferencesCurrentApplication`, идентификатор из `Info.plist`).
//!
//! Только принудительные значения (`CFPreferencesAppValueIsForced`): свои `defaults write` пользователя политикой не
//! считаются — как и HKCU-значения вне `Policies`, которые пользователь пишет сам.

use objc2_core_foundation::{
    CFBoolean, CFNumber, CFPreferencesAppValueIsForced, CFPreferencesCopyAppValue, CFString,
    kCFPreferencesCurrentApplication,
};

/// Значение `key` управляемых настроек приложения строкой: строка как есть, логическое — `1`/`0`, число — десятичное
/// (как `REG_SZ`/`REG_DWORD` политики Windows). Нет, не принудительное или другой тип — `None`. Микросекунды, любой поток.
pub fn get(key: &str) -> Option<String> {
    // SAFETY: неизменяемая константа CoreFoundation, живёт весь процесс.
    let app = unsafe { kCFPreferencesCurrentApplication };
    let name = CFString::from_str(key);
    if !CFPreferencesAppValueIsForced(&name, app) {
        return None;
    }
    let value = CFPreferencesCopyAppValue(&name, app)?;
    if let Some(s) = value.downcast_ref::<CFString>() {
        return Some(s.to_string());
    }
    if let Some(b) = value.downcast_ref::<CFBoolean>() {
        return Some(if b.as_bool() { "1" } else { "0" }.to_owned());
    }
    value
        .downcast_ref::<CFNumber>()
        .and_then(CFNumber::as_i64)
        .map(|n| n.to_string())
}

#[cfg(test)]
mod tests {
    /// Значения, которого никто не задавал, нет.
    #[test]
    fn missing_value_is_none() {
        assert_eq!(super::get("PetsShellTestNothingHere"), None);
    }
}
