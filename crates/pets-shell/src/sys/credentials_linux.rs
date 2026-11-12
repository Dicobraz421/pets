//! Секрет на Linux — Secret Service (freedesktop, D-Bus сеанса: GNOME Keyring, KWallet 6, KeePassXC) через `zbus`:
//! элемент коллекции по умолчанию (`aliases/default`, «Вход») с атрибутами `xdg:schema=org.pets.Credential`,
//! `service=<target>`, `username=<user>`. Службы нет — ошибка `NotFound` с понятным текстом; на диск секрет не пишется.
//! Сеанс — алгоритм `plain` (секрет идёт по сокету D-Bus своего пользователя, как у libsecret без шифрования
//! транспорта). Заблокированная коллекция — запрос разблокировки службы (окно пароля), ждём ответа пользователя.
//! Секрет не попадает в ошибки и журнал; копии в памяти затираются.

use std::collections::HashMap;

use zbus::export::futures_core::Stream as _;
use zbus::export::serde;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue as OwnedVariant, Value as Variant};
use zbus::{Connection, MessageStream};

use crate::ShellError;

const DEST: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const SERVICE: &str = "org.freedesktop.Secret.Service";
const COLLECTION: &str = "org.freedesktop.Secret.Collection";
const ITEM: &str = "org.freedesktop.Secret.Item";
const PROMPT: &str = "org.freedesktop.Secret.Prompt";
const SCHEMA: &str = "org.pets.Credential";

/// Секрет Secret Service: сеанс, параметры, значение, тип содержимого.
type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

fn os(what: &'static str, e: zbus::Error) -> ShellError {
    let missing = match &e {
        zbus::Error::FDO(f) => matches!(
            **f,
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)
        ),
        zbus::Error::MethodError(name, _, _) => name.as_str().ends_with(".ServiceUnknown"),
        _ => false,
    };
    let source = if missing {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no Secret Service on the session bus (GNOME Keyring, KWallet or KeePassXC)",
        )
    } else {
        std::io::Error::other(e.to_string())
    };
    ShellError::Os { what, source }
}

/// Соединение с Secret Service и открытый сеанс `plain`.
struct Client {
    conn: Connection,
    session: OwnedObjectPath,
}

impl Client {
    async fn open() -> Result<Self, ShellError> {
        let conn = Connection::session().await.map_err(|e| {
            let source = std::io::Error::new(std::io::ErrorKind::NotFound, format!("no D-Bus session bus: {e}"));
            ShellError::Os {
                what: "Secret Service",
                source,
            }
        })?;
        let reply = conn
            .call_method(
                Some(DEST),
                SERVICE_PATH,
                Some(SERVICE),
                "OpenSession",
                &("plain", Variant::from("")),
            )
            .await
            .map_err(|e| os("OpenSession", e))?;
        let (_, session): (OwnedVariant, OwnedObjectPath) =
            reply.body().deserialize().map_err(|e| os("OpenSession", e))?;
        Ok(Self { conn, session })
    }

    async fn call<B, R>(&self, path: &str, iface: &str, method: &'static str, body: &B) -> Result<R, ShellError>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
        R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
    {
        let reply = self
            .conn
            .call_method(Some(DEST), path, Some(iface), method, body)
            .await
            .map_err(|e| os(method, e))?;
        reply.body().deserialize().map_err(|e| os(method, e))
    }

    /// Запрос службы (`/` — не нужен): показать и дождаться `Completed`. Отказ пользователя — ошибка.
    async fn prompt(&self, prompt: &OwnedObjectPath) -> Result<(), ShellError> {
        if prompt.as_str() == "/" {
            return Ok(());
        }
        let rule = format!(
            "type='signal',interface='{PROMPT}',member='Completed',path='{}'",
            prompt.as_str()
        );
        let mut stream = MessageStream::for_match_rule(rule.as_str(), &self.conn, None)
            .await
            .map_err(|e| os("Prompt", e))?;
        let () = self.call(prompt.as_str(), PROMPT, "Prompt", &("",)).await?;
        let msg = std::future::poll_fn(|cx| std::pin::Pin::new(&mut stream).poll_next(cx))
            .await
            .ok_or_else(|| os("Prompt", zbus::Error::Failure("prompt stream ended".into())))?
            .map_err(|e| os("Prompt", e))?;
        let (dismissed, _): (bool, OwnedVariant) = msg.body().deserialize().map_err(|e| os("Prompt", e))?;
        if dismissed {
            return Err(ShellError::Invalid {
                what: "Secret Service",
                reason: "unlock prompt dismissed",
            });
        }
        Ok(())
    }

    /// Элементы записи `target`; заблокированные — разблокировать (запрос пользователю).
    async fn items(&self, target: &str) -> Result<Vec<OwnedObjectPath>, ShellError> {
        let attrs = HashMap::from([("xdg:schema", SCHEMA), ("service", target)]);
        let (mut unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
            self.call(SERVICE_PATH, SERVICE, "SearchItems", &(attrs,)).await?;
        if !locked.is_empty() {
            let (done, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) =
                self.call(SERVICE_PATH, SERVICE, "Unlock", &(&locked,)).await?;
            self.prompt(&prompt).await?;
            // Разблокированные запросом не приходят в `done`: после запроса — все прежде заблокированные.
            unlocked.extend(if prompt.as_str() == "/" { done } else { locked });
        }
        Ok(unlocked)
    }

    /// Коллекция по умолчанию; нет (чистая система) — создать («Вход», запрос пароля новой связки).
    async fn default_collection(&self) -> Result<OwnedObjectPath, ShellError> {
        let path: OwnedObjectPath = self.call(SERVICE_PATH, SERVICE, "ReadAlias", &("default",)).await?;
        if path.as_str() != "/" {
            return Ok(path);
        }
        let props = HashMap::from([("org.freedesktop.Secret.Collection.Label", Variant::from("Login"))]);
        let (path, prompt): (OwnedObjectPath, OwnedObjectPath) = self
            .call(SERVICE_PATH, SERVICE, "CreateCollection", &(props, "default"))
            .await?;
        self.prompt(&prompt).await?;
        if path.as_str() != "/" {
            return Ok(path);
        }
        // Создана через запрос: путь — по псевдониму.
        self.call(SERVICE_PATH, SERVICE, "ReadAlias", &("default",)).await
    }

    async fn delete(&self, target: &str) -> Result<(), ShellError> {
        for item in self.items(target).await? {
            let prompt: OwnedObjectPath = self.call(item.as_str(), ITEM, "Delete", &()).await?;
            self.prompt(&prompt).await?;
        }
        Ok(())
    }
}

fn block_on<T>(f: impl Future<Output = Result<T, ShellError>>) -> Result<T, ShellError> {
    zbus::block_on(f)
}

/// Записать (заменить) секрет `secret` пользователя `user` под именем `target`.
pub fn write(target: &str, user: &str, secret: &str) -> Result<(), ShellError> {
    block_on(async {
        let client = Client::open().await?;
        // Прежняя запись могла быть с другим пользователем: имя одно — запись одна.
        client.delete(target).await?;
        let collection = client.default_collection().await?;
        let attrs = HashMap::from([("xdg:schema", SCHEMA), ("service", target), ("username", user)]);
        let props = HashMap::from([
            ("org.freedesktop.Secret.Item.Label", Variant::from(target)),
            ("org.freedesktop.Secret.Item.Attributes", Variant::from(attrs)),
        ]);
        let mut value = secret.as_bytes().to_vec();
        let session: &ObjectPath<'_> = &client.session;
        let body = (props, (session, Vec::<u8>::new(), &value, "text/plain"), true);
        let created: Result<(OwnedObjectPath, OwnedObjectPath), _> =
            client.call(collection.as_str(), COLLECTION, "CreateItem", &body).await;
        drop(body);
        value.fill(0);
        let (_, prompt) = created?;
        client.prompt(&prompt).await
    })
}

/// Секрет записи `target`; `Ok(None)` — такой записи нет.
pub fn read(target: &str) -> Result<Option<String>, ShellError> {
    block_on(async {
        let client = Client::open().await?;
        let Some(item) = client.items(target).await?.into_iter().next() else {
            return Ok(None);
        };
        let session: &ObjectPath<'_> = &client.session;
        let (_, _, value, _): Secret = client.call(item.as_str(), ITEM, "GetSecret", &(session,)).await?;
        String::from_utf8(value).map(Some).map_err(|e| {
            e.into_bytes().fill(0);
            ShellError::Invalid {
                what: "GetSecret",
                reason: "secret is not UTF-8",
            }
        })
    })
}

/// Удалить запись; нет записи — не ошибка.
pub fn delete(target: &str) -> Result<(), ShellError> {
    block_on(async { Client::open().await?.delete(target).await })
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

    /// Нет службы (шина сеанса без `org.freedesktop.secrets`, как в CI) — ошибка `NotFound`, не паника и не файл.
    #[test]
    fn credentials_without_service_is_not_found() {
        match super::read("Pets:test-missing") {
            Ok(None) => {} // служба есть, записи нет
            Ok(Some(_)) => panic!("unexpected secret"),
            Err(crate::ShellError::Os { source, .. }) => {
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound, "{source}")
            }
            Err(e) => panic!("{e}"),
        }
    }
}
