//! Linux: приложение по умолчанию для схемы адресов (`mailto`) — тип `x-scheme-handler/<схема>` в `mimeapps.list`
//! пользователя через `xdg-utils` (`xdg-settings`, запасной путь — `xdg-mime`). Нужен установленный `.desktop` этого
//! исполняемого файла с `MimeType=x-scheme-handler/<схема>;` и `Exec=… %u` (пакет приложения); без него — ошибка, как
//! на macOS вне `.app`.

use crate::ShellError;
use crate::sys::desktop;

/// Идентификатор `.desktop` этого процесса (`postman.desktop`); не установлен (`cargo run`) — `None`. Аналог
/// идентификатора пакета macOS.
pub fn bundle_id() -> Option<String> {
    desktop::desktop_id()
}

fn mime(scheme: &str) -> String {
    format!("x-scheme-handler/{scheme}")
}

/// Сделать это приложение обработчиком схемы `scheme` по умолчанию.
pub fn set_default(scheme: &str) -> Result<(), ShellError> {
    let id = bundle_id().ok_or(ShellError::Invalid {
        what: "xdg-settings",
        reason: "no .desktop entry for this executable",
    })?;
    match desktop::run("xdg-settings", &["set", "default-url-scheme-handler", scheme, &id]) {
        Ok(_) => Ok(()),
        Err(e) => {
            tracing::warn!(target: "pets_shell", "xdg-settings: {e}; falling back to xdg-mime");
            desktop::run("xdg-mime", &["default", &id, &mime(scheme)]).map(drop)
        }
    }
}

/// Идентификатор `.desktop` обработчика схемы по умолчанию; нет или `xdg-mime` нет — `None`.
pub fn default_handler(scheme: &str) -> Option<String> {
    desktop::run("xdg-mime", &["query", "default", &mime(scheme)])
        .ok()
        .filter(|id| !id.is_empty())
}

/// Обработчик схемы по умолчанию — это приложение.
pub fn is_default(scheme: &str) -> bool {
    match (bundle_id(), default_handler(scheme)) {
        (Some(me), Some(h)) => me == h,
        _ => false,
    }
}
