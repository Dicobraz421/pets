//! Строковые значения реестра: запись и удаление — в разделе пользователя (без прав администратора), чтение — и из
//! раздела машины (политики). `name` — `None`: значение по умолчанию ключа.

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
};

use super::{os_code, to_wide};
use crate::ShellError;

/// Раздел реестра.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    /// `HKEY_CURRENT_USER`.
    User,
    /// `HKEY_LOCAL_MACHINE` — только чтение.
    Machine,
}

/// Записать строку (`REG_SZ`) в `HKCU\<path>`, создав ключ.
pub fn set_string(path: &str, name: Option<&str>, value: &str) -> Result<(), ShellError> {
    let wpath = to_wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: строка пути живёт до конца вызова; `key` — выходной параметр.
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wpath.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(os_code("RegCreateKeyExW", rc));
    }
    let wname = name.map(to_wide);
    let data = to_wide(value);
    let bytes = u32::try_from(data.len() * 2).map_err(|_| ShellError::Invalid {
        what: "RegSetValueExW",
        reason: "value is too long",
    })?;
    // SAFETY: ключ открыт выше; имя — UTF-16 с нулём или null; данные — UTF-16 с нулём, размер в байтах.
    let rc = unsafe {
        RegSetValueExW(
            key,
            wname.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
            0,
            REG_SZ,
            data.as_ptr().cast(),
            bytes,
        )
    };
    // SAFETY: ключ открыт выше и больше не нужен.
    unsafe { RegCloseKey(key) };
    if rc == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(os_code("RegSetValueExW", rc))
    }
}

/// Строковое значение (`REG_SZ`); нет ключа или значения — `None`.
pub fn get_string(hive: Hive, path: &str, name: Option<&str>) -> Option<String> {
    let root = match hive {
        Hive::User => HKEY_CURRENT_USER,
        Hive::Machine => HKEY_LOCAL_MACHINE,
    };
    let wpath = to_wide(path);
    let wname = name.map(to_wide);
    let mut buf = vec![0u16; 1024];
    let mut size = u32::try_from(buf.len() * 2).ok()?;
    // SAFETY: буфер и его размер в байтах; строки живут до конца вызова.
    let rc = unsafe {
        RegGetValueW(
            root,
            wpath.as_ptr(),
            wname.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Удалить значение в `HKCU\<path>`; нет ключа или значения — не ошибка.
pub fn delete_value(path: &str, name: &str) -> Result<(), ShellError> {
    let (wpath, wname) = (to_wide(path), to_wide(name));
    // SAFETY: строки живут до конца вызова.
    let rc = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, wpath.as_ptr(), wname.as_ptr()) };
    if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(os_code("RegDeleteKeyValueW", rc))
    }
}

/// Удалить ключ `HKCU\<path>` со всем содержимым; нет — не ошибка.
pub fn delete_tree(path: &str) -> Result<(), ShellError> {
    let wpath = to_wide(path);
    // SAFETY: строка пути живёт до конца вызова.
    let rc = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wpath.as_ptr()) };
    if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(os_code("RegDeleteTreeW", rc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Чтение несуществующего значения — `None`, удаление — не ошибка; запись читается обратно.
    #[test]
    fn registry_round_trip() {
        let path = format!(r"Software\Pets\test-registry-{}", std::process::id());
        assert_eq!(get_string(Hive::User, &path, Some("x")), None);
        assert_eq!(
            get_string(Hive::Machine, r"Software\Pets-test-nothing-here", None),
            None
        );
        delete_value(&path, "x").unwrap();
        set_string(&path, Some("x"), "значение").unwrap();
        set_string(&path, None, "по умолчанию").unwrap();
        assert_eq!(get_string(Hive::User, &path, Some("x")).as_deref(), Some("значение"));
        assert_eq!(get_string(Hive::User, &path, None).as_deref(), Some("по умолчанию"));
        delete_value(&path, "x").unwrap();
        assert_eq!(get_string(Hive::User, &path, Some("x")), None);
        delete_value(&path, "x").unwrap();
        delete_tree(&path).unwrap();
        delete_tree(&path).unwrap();
        assert_eq!(get_string(Hive::User, &path, None), None);
    }
}
