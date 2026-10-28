//! Строковые значения реестра: запись и удаление — в разделе пользователя (без прав администратора), чтение — и из
//! раздела машины (политики). `name` — `None`: значение по умолчанию ключа. Без `unsafe`: `windows-registry`.

use std::io;

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::core::Error;
use windows_registry::{CURRENT_USER, LOCAL_MACHINE};

/// Раздел реестра.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    /// `HKEY_CURRENT_USER`.
    User,
    /// `HKEY_LOCAL_MACHINE` — только чтение.
    Machine,
}

fn os(what: &str, path: &str, e: Error) -> io::Error {
    io::Error::other(format!("{what} {path}: {}", e.message()))
}

/// Записать строку (`REG_SZ`) в `HKCU\<path>`, создав ключ.
pub fn set_string(path: &str, name: Option<&str>, value: &str) -> io::Result<()> {
    CURRENT_USER
        .create(path)
        .and_then(|key| key.set_string(name.unwrap_or(""), value))
        .map_err(|e| os("set", path, e))
}

/// Строковое значение; нет ключа или значения — `None`.
pub fn get_string(hive: Hive, path: &str, name: Option<&str>) -> Option<String> {
    let root = match hive {
        Hive::User => CURRENT_USER,
        Hive::Machine => LOCAL_MACHINE,
    };
    root.open(path).and_then(|key| key.get_string(name.unwrap_or(""))).ok()
}

/// Удалить значение в `HKCU\<path>`; нет ключа или значения — не ошибка.
pub fn delete_value(path: &str, name: &str) -> io::Result<()> {
    let key = match CURRENT_USER.options().read().write().open(path) {
        Ok(key) => key,
        Err(e) if not_found(&e) => return Ok(()),
        Err(e) => return Err(os("open", path, e)),
    };
    match key.remove_value(name) {
        Err(e) if !not_found(&e) => Err(os("delete", path, e)),
        _ => Ok(()),
    }
}

/// Удалить ключ `HKCU\<path>` со всем содержимым; нет — не ошибка.
pub fn delete_tree(path: &str) -> io::Result<()> {
    match CURRENT_USER.remove_tree(path) {
        Err(e) if !not_found(&e) => Err(os("delete", path, e)),
        _ => Ok(()),
    }
}

fn not_found(e: &Error) -> bool {
    e.code() == ERROR_FILE_NOT_FOUND.to_hresult()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Чтение несуществующего значения — `None`, удаление — не ошибка; запись читается обратно.
    #[test]
    fn pets_registry_round_trip() {
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
