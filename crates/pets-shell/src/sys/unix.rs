//! macOS и Linux: единственный экземпляр — замок файла `Pets.<app>.lock` (`flock`) и unix-сокет `Pets.<app>.sock`
//! вместо именованного канала. Каталог — свой у пользователя: macOS — `$TMPDIR` (права 0700), Linux —
//! `$XDG_RUNTIME_DIR` (`/run/user/<uid>`, 0700). Переменной нет — не общий `/tmp`, а свой каталог `pets-<uid>` в нём
//! (0700, владелец — этот пользователь; чужой или открытый другим — ошибка, предупреждение в журнал). Ошибка замка —
//! ошибка, как на Windows (PAR-03).
//!
//! macOS: ссылки `mailto:` и файлы из Finder приходят первому экземпляру событием Apple (`on_open_urls` gpui), а не
//! вторым процессом; сокет — для запуска из терминала и `open -n`. Linux: ссылку `mailto:` из `.desktop` (`%u`)
//! передаёт второй процесс — через сокет.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::ShellError;
use crate::message::MAX_LEN;

/// Сколько второй экземпляр ждёт сокет первого (тот мог взять замок и ещё не открыть сокет).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Пауза между попытками, пока сокета нет.
const RETRY: Duration = Duration::from_millis(50);

/// Замок имени и сокет первого экземпляра: живут до конца процесса.
#[derive(Debug)]
pub(crate) struct Guard {
    _lock: File,
    listener: UnixListener,
}

/// Переменная с личным каталогом выполнения пользователя.
#[cfg(target_os = "macos")]
const RUNTIME_VAR: &str = "TMPDIR";
#[cfg(target_os = "linux")]
const RUNTIME_VAR: &str = "XDG_RUNTIME_DIR";

/// Каталог замка и сокета: `$TMPDIR` (macOS) или `$XDG_RUNTIME_DIR` (Linux), абсолютный путь; иначе свой каталог
/// [`private_dir`] во временном каталоге системы.
fn dir() -> Result<PathBuf, ShellError> {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    if let Some(d) = DIR.get() {
        return Ok(d.clone());
    }
    let d = match std::env::var_os(RUNTIME_VAR).map(PathBuf::from) {
        Some(d) if d.is_absolute() => d,
        _ => {
            tracing::warn!(target: "pets_shell", "{RUNTIME_VAR} is not set: instance socket in a private temp dir");
            private_dir(&std::env::temp_dir())?
        }
    };
    Ok(DIR.get_or_init(|| d).clone())
}

/// `base/pets-<uid>` с правами 0700: создать или проверить, что это каталог (не ссылка) этого пользователя, закрытый
/// для других, — иначе в общем каталоге чужой мог подложить свой.
fn private_dir(base: &Path) -> Result<PathBuf, ShellError> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
    // SAFETY: без аргументов, не может завершиться неудачей.
    let uid = unsafe { libc::getuid() };
    let dir = base.join(format!("pets-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(os("instance directory", e)),
        _ => {}
    }
    let meta = std::fs::symlink_metadata(&dir).map_err(|e| os("instance directory", e))?;
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(os(
            "instance directory",
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{} is not a private directory of this user", dir.display()),
            ),
        ));
    }
    Ok(dir)
}

fn path(app: &str, ext: &str) -> Result<PathBuf, ShellError> {
    Ok(dir()?.join(format!("Pets.{app}.{ext}")))
}

fn os(what: &'static str, source: std::io::Error) -> ShellError {
    ShellError::Os { what, source }
}

/// Взять имя: `Some` — этот процесс первый, `None` — имя держит другой.
pub(crate) fn acquire(app: &str) -> Result<Option<Guard>, ShellError> {
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path(app, "lock")?)
        .map_err(|e| os("instance lock", e))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(e)) => return Err(os("flock", e)),
    }
    // Замок наш — сокет прошлого запуска (упал, не убрал) больше никому не нужен.
    let sock = path(app, "sock")?;
    match std::fs::remove_file(&sock) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(os("instance socket", e)),
        _ => {}
    }
    let listener = UnixListener::bind(&sock).map_err(|e| os("bind", e))?;
    Ok(Some(Guard { _lock: lock, listener }))
}

/// Подключиться к сокету первого экземпляра (повторы до `CONNECT_TIMEOUT`) и передать сообщение.
pub(crate) fn send(app: &str, message: &[u8]) -> Result<(), ShellError> {
    let sock = path(app, "sock")?;
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let mut stream = loop {
        match UnixStream::connect(&sock) {
            Ok(s) => break s,
            Err(_) if Instant::now() < deadline => std::thread::sleep(RETRY),
            Err(_) => return Err(ShellError::NoServer(CONNECT_TIMEOUT.as_millis() as u32)),
        }
    };
    stream.write_all(message).map_err(|e| os("write", e))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|e| os("shutdown", e))
}

/// Цикл сервера: подключение → сообщение целиком (до закрытия записи) → `on_message`. Не возвращается.
pub(crate) fn serve(guard: Guard, on_message: &dyn Fn(Vec<u8>)) {
    for stream in guard.listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(target: "pets_shell", "accept: {e}");
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let mut message = Vec::new();
        let read = (&mut stream).take(MAX_LEN as u64 + 1).read_to_end(&mut message);
        match read {
            Ok(_) if message.len() > MAX_LEN => {
                let e = ShellError::TooLarge(message.len());
                tracing::warn!(target: "pets_shell", "сообщение второго экземпляра: {e}");
            }
            Ok(_) => on_message(message),
            Err(e) => tracing::warn!(target: "pets_shell", "сообщение второго экземпляра: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    /// Второй вызов в том же процессе (замок уже взят) передаёт сообщение первому через сокет.
    #[test]
    fn second_passes_message_to_first() {
        let app = format!("PetsShellTest.{}", std::process::id());
        let first = super::acquire(&app).unwrap().expect("первый — владелец имени");
        assert!(super::acquire(&app).unwrap().is_none());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || super::serve(first, &move |m| tx.send(m).unwrap()));
        super::send(&app, b"hello").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), b"hello");
    }

    /// Без каталога выполнения — свой каталог 0700; открытый для других — ошибка, а не общий каталог.
    #[test]
    fn private_dir_is_owned_and_closed() {
        use std::os::unix::fs::PermissionsExt as _;
        let base = std::env::temp_dir().join(format!("pets-shell-dir-test-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let dir = super::private_dir(&base).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        assert_eq!(super::private_dir(&base).unwrap(), dir);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(super::private_dir(&base).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
