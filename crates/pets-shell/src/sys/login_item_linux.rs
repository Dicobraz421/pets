//! Запуск при входе в систему на Linux — XDG Autostart: `~/.config/autostart/<id>.desktop` (`$XDG_CONFIG_HOME`).
//! `<id>` — `.desktop` приложения (`url_scheme::bundle_id`), не установлено — имя исполняемого файла. Пользователь
//! может выключить запись в настройках среды (`Hidden=true`, `X-GNOME-Autostart-enabled=false`): тогда
//! [`State::RequiresApproval`]; [`set`]`(true)` включает её снова.

use std::path::PathBuf;

use crate::ShellError;
use crate::sys::desktop;

/// Состояние объекта входа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Записи автозапуска нет.
    Off,
    /// Запускается при входе.
    On,
    /// Запись есть, но выключена пользователем.
    RequiresApproval,
}

fn exe() -> Result<PathBuf, ShellError> {
    std::env::current_exe().map_err(|source| ShellError::Os {
        what: "current_exe",
        source,
    })
}

/// Путь записи автозапуска этого приложения.
fn entry_path() -> Result<PathBuf, ShellError> {
    let dir = desktop::config_home().ok_or(ShellError::Invalid {
        what: "autostart",
        reason: "neither XDG_CONFIG_HOME nor HOME is set",
    })?;
    let id = match desktop::desktop_id() {
        Some(id) => id,
        None => {
            let exe = exe()?;
            let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned());
            format!("{}.desktop", stem.unwrap_or_else(|| "pets".into()))
        }
    };
    Ok(dir.join("autostart").join(id))
}

/// Состояние по тексту записи.
fn state_of(text: &str) -> State {
    let off = desktop::entry_value(text, "Hidden").is_some_and(|v| v == "true")
        || desktop::entry_value(text, "X-GNOME-Autostart-enabled").is_some_and(|v| v == "false");
    if off { State::RequiresApproval } else { State::On }
}

/// Текущее состояние. Любой поток.
pub fn state() -> State {
    match entry_path().map(std::fs::read_to_string) {
        Ok(Ok(text)) => state_of(&text),
        _ => State::Off,
    }
}

/// Текст записи: имя — из `.desktop` приложения, иначе имя файла.
fn entry_text(name: &str, exe: &std::path::Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop::exec_line(exe, "")
    )
}

/// Включить (записать) или выключить (удалить) запуск при входе.
pub fn set(on: bool) -> Result<(), ShellError> {
    let path = entry_path()?;
    let os = |what, source| ShellError::Os { what, source };
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(os("autostart remove", e)),
            _ => Ok(()),
        };
    }
    let exe = exe()?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| os("autostart directory", e))?;
    }
    std::fs::write(&path, entry_text(&name, &exe)).map_err(|e| os("autostart write", e))
}

/// Общего окна «Автозапуск» во freedesktop нет: открыть каталог `autostart` файловым менеджером (`xdg-open`).
pub fn open_settings() {
    let Some(dir) = entry_path().ok().and_then(|p| p.parent().map(PathBuf::from)) else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(target: "pets_shell", "autostart directory: {e}");
        return;
    }
    let spawned = std::process::Command::new("xdg-open")
        .arg(&dir)
        .stdin(std::process::Stdio::null())
        .spawn();
    if let Err(e) = spawned {
        tracing::warn!(target: "pets_shell", "xdg-open autostart: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::State;

    /// Своя запись — `On`; выключенная пользователем — `RequiresApproval`.
    #[test]
    fn autostart_state_of_entry() {
        let text = super::entry_text("postman", std::path::Path::new("/opt/Post man/postman"));
        assert_eq!(super::state_of(&text), State::On);
        assert!(text.contains("Exec=\"/opt/Post man/postman\"\n"));
        let off = text.replace("X-GNOME-Autostart-enabled=true", "X-GNOME-Autostart-enabled=false");
        assert_eq!(super::state_of(&off), State::RequiresApproval);
        assert_eq!(
            super::state_of(&format!("{text}Hidden=true\n")),
            State::RequiresApproval
        );
    }
}
