//! Секрет на Linux — Secret Service (freedesktop, D-Bus сеанса: GNOME Keyring, KWallet 6, KeePassXC) через `oo7`
//! 0.6, только D-Bus-бэкенд (`oo7::dbus::Service`; не `oo7::Keyring` — тот в песочнице уходит в файл, а на диск нельзя).
//! Сеанс зашифрован (`Service::encrypted`, DH + AES). Элемент коллекции по умолчанию с атрибутами
//! `xdg:schema=org.pets.Credential`, `service=<target>`, `username=<user>`. Службы нет — ошибка `NotFound` с понятным
//! текстом. Заблокированная связка — запрос разблокировки службы, ждём ответа пользователя. Секрет не попадает в ошибки
//! и журнал; копии в памяти затирает `oo7` (`Zeroizing`).

use oo7::dbus::{Collection, Error, Service};

use crate::ShellError;

const SCHEMA: &str = "org.pets.Credential";

fn os(what: &'static str, e: Error) -> ShellError {
    let missing = match &e {
        Error::ZBus(zbus::Error::FDO(f)) => {
            matches!(
                **f,
                zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
            )
        }
        Error::ZBus(zbus::Error::MethodError(name, _, _)) => name.as_str().ends_with(".ServiceUnknown"),
        Error::ZBus(zbus::Error::Address(_) | zbus::Error::InputOutput(_)) => true,
        _ => false,
    };
    if matches!(e, Error::Dismissed) {
        return ShellError::Invalid {
            what: "Secret Service",
            reason: "unlock prompt dismissed",
        };
    }
    let source = if missing {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no Secret Service on the session bus (GNOME Keyring, KWallet or KeePassXC): {e}"),
        )
    } else {
        std::io::Error::other(e.to_string())
    };
    ShellError::Os { what, source }
}

/// Коллекция по умолчанию (нет — создаётся службой) разблокированная: заблокирована — запрос пользователю.
async fn collection() -> Result<Collection, ShellError> {
    // Не подключились или не открыли сеанс — службы для нас нет (нет шины сеанса, нет `org.freedesktop.secrets`).
    let service = Service::encrypted().await.map_err(|e| ShellError::Os {
        what: "Secret Service",
        source: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no Secret Service on the session bus (GNOME Keyring, KWallet or KeePassXC): {e}"),
        ),
    })?;
    let collection = service
        .default_collection()
        .await
        .map_err(|e| os("Secret Service default collection", e))?;
    if collection
        .is_locked()
        .await
        .map_err(|e| os("Secret Service unlock", e))?
    {
        collection
            .unlock(None)
            .await
            .map_err(|e| os("Secret Service unlock", e))?;
    }
    Ok(collection)
}

async fn delete_in(collection: &Collection, target: &str) -> Result<(), ShellError> {
    let items = collection
        .search_items(&[("xdg:schema", SCHEMA), ("service", target)])
        .await
        .map_err(|e| os("SearchItems", e))?;
    for item in items {
        item.delete(None).await.map_err(|e| os("Item.Delete", e))?;
    }
    Ok(())
}

/// Записать (заменить) секрет `secret` пользователя `user` под именем `target` — за один шаг: `CreateItem` с
/// `replace=true` заменяет элемент с теми же атрибутами (как `CredWriteW`); сбой — прежний секрет цел. Записи того же
/// имени с другим пользователем удаляются только после успешной записи: имя одно — запись одна.
pub fn write(target: &str, user: &str, secret: &str) -> Result<(), ShellError> {
    zbus::block_on(async {
        let collection = collection().await?;
        let attrs = [("xdg:schema", SCHEMA), ("service", target), ("username", user)];
        let created = collection
            .create_item(target, &attrs, oo7::Secret::text(secret), true, None)
            .await
            .map_err(|e| os("CreateItem", e))?;
        let items = collection
            .search_items(&[("xdg:schema", SCHEMA), ("service", target)])
            .await
            .map_err(|e| os("SearchItems", e))?;
        for item in items.iter().filter(|i| i.path() != created.path()) {
            item.delete(None).await.map_err(|e| os("Item.Delete", e))?;
        }
        Ok(())
    })
}

/// Секрет записи `target`; `Ok(None)` — такой записи нет.
pub fn read(target: &str) -> Result<Option<String>, ShellError> {
    zbus::block_on(async {
        let collection = collection().await?;
        let items = collection
            .search_items(&[("xdg:schema", SCHEMA), ("service", target)])
            .await
            .map_err(|e| os("SearchItems", e))?;
        let Some(item) = items.into_iter().next() else {
            return Ok(None);
        };
        let secret = item.secret().await.map_err(|e| os("GetSecret", e))?;
        std::str::from_utf8(secret.as_bytes())
            .map(|s| Some(s.to_owned()))
            .map_err(|_| ShellError::Invalid {
                what: "GetSecret",
                reason: "secret is not UTF-8",
            })
    })
}

/// Удалить запись; нет записи — не ошибка.
pub fn delete(target: &str) -> Result<(), ShellError> {
    zbus::block_on(async { delete_in(&collection().await?, target).await })
}

#[cfg(test)]
mod tests {
    /// Пишет, читает и удаляет пробную запись в связке ключей этого пользователя.
    #[test]
    #[ignore = "writes to the Secret Service of the current user"]
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

    /// Нет службы (нет шины сеанса или на ней нет `org.freedesktop.secrets`) — ошибка `NotFound`, не паника и не
    /// файл; служба есть — записи нет.
    #[test]
    fn credentials_without_service_is_not_found() {
        match super::read("Pets:test-missing") {
            Ok(None) => {}
            Ok(Some(_)) => panic!("unexpected secret"),
            Err(crate::ShellError::Os { source, .. }) => {
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound, "{source}")
            }
            Err(e) => panic!("{e}"),
        }
    }
}
