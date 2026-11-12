//! Секрет в Связке ключей входа macOS (Security.framework, крейт `security-framework`): «пароль приложения»
//! (`kSecClassGenericPassword`), служба — имя записи (`target`), учётная запись — `user`. Доступ — у этого приложения
//! (ACL по подписи; при ad-hoc подписи новая сборка один раз спросит разрешение). Секрет не попадает в ошибки — только
//! код `OSStatus`; копия в памяти при записи затирается.

use security_framework::base::Error;
use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
use security_framework::passwords::set_generic_password;

use crate::ShellError;

/// `errSecItemNotFound`.
const NOT_FOUND: i32 = -25300;

fn os(what: &'static str, e: Error) -> ShellError {
    ShellError::Os {
        what,
        source: std::io::Error::other(format!("OSStatus {}", e.code())),
    }
}

fn search(target: &str) -> ItemSearchOptions {
    let mut options = ItemSearchOptions::new();
    options.class(ItemClass::generic_password()).service(target);
    options
}

/// Записать (заменить) секрет `secret` пользователя `user` под именем `target`.
pub fn write(target: &str, user: &str, secret: &str) -> Result<(), ShellError> {
    // Прежняя запись могла быть с другим пользователем: служба одна — запись одна.
    delete(target)?;
    let mut blob = secret.as_bytes().to_vec();
    let result = set_generic_password(target, user, &blob).map_err(|e| os("SecItemAdd", e));
    blob.fill(0);
    result
}

/// Секрет записи `target`; `Ok(None)` — такой записи нет.
pub fn read(target: &str) -> Result<Option<String>, ShellError> {
    let mut options = search(target);
    options.load_data(true).limit(Limit::Max(1));
    let found = match options.search() {
        Ok(found) => found,
        Err(e) if e.code() == NOT_FOUND => return Ok(None),
        Err(e) => return Err(os("SecItemCopyMatching", e)),
    };
    let Some(bytes) = found.into_iter().find_map(|r| match r {
        SearchResult::Data(d) => Some(d),
        _ => None,
    }) else {
        return Ok(None);
    };
    String::from_utf8(bytes).map(Some).map_err(|_| ShellError::Invalid {
        what: "SecItemCopyMatching",
        reason: "secret is not UTF-8",
    })
}

/// Удалить запись; нет записи — не ошибка.
pub fn delete(target: &str) -> Result<(), ShellError> {
    match search(target).delete() {
        Err(e) if e.code() != NOT_FOUND => Err(os("SecItemDelete", e)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    /// Пишет, читает и удаляет пробную запись в Связке ключей этого пользователя.
    #[test]
    #[ignore = "writes to the login keychain of the current user"]
    fn credentials_round_trip() {
        let target = format!("Pets:test:{}", std::process::id());
        assert_eq!(super::read(&target).unwrap(), None);
        super::write(&target, "user", "dummy секрет").unwrap();
        assert_eq!(super::read(&target).unwrap().as_deref(), Some("dummy секрет"));
        super::write(&target, "other", "second").unwrap();
        assert_eq!(super::read(&target).unwrap().as_deref(), Some("second"));
        super::delete(&target).unwrap();
        assert_eq!(super::read(&target).unwrap(), None);
        super::delete(&target).unwrap();
    }

    /// Нет записи — `None` и удаление без ошибки (запись не создаётся, Связка ключей не спрашивает).
    #[test]
    fn credentials_missing_is_none() {
        let target = format!(
            "Pets:test-missing:{}:{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        );
        assert_eq!(super::read(&target).unwrap(), None);
        super::delete(&target).unwrap();
    }
}
