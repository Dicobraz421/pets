//! macOS: единственный экземпляр — замок файла `$TMPDIR/Pets.<app>.lock` (`flock`) и unix-сокет
//! `$TMPDIR/Pets.<app>.sock` вместо именованного канала; `$TMPDIR` — свой у пользователя (права 0700). Журнала событий
//! ОС нет: ошибки запуска — в stderr (`init_logging`).
//!
//! Ссылки `mailto:` и файлы из Finder приходят первому экземпляру событием Apple (`on_open_urls` gpui), а не вторым
//! процессом; сокет — для запуска из терминала и `open -n`.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
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

fn path(app: &str, ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("Pets.{app}.{ext}"))
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
        .open(path(app, "lock"))
        .map_err(|e| os("instance lock", e))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(e)) => return Err(os("flock", e)),
    }
    // Замок наш — сокет прошлого запуска (упал, не убрал) больше никому не нужен.
    let sock = path(app, "sock");
    match std::fs::remove_file(&sock) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(os("instance socket", e)),
        _ => {}
    }
    let listener = UnixListener::bind(&sock).map_err(|e| os("bind", e))?;
    Ok(Some(Guard { _lock: lock, listener }))
}

/// Подключиться к сокету первого экземпляра (повторы до `CONNECT_TIMEOUT`) и передать сообщение.
pub(crate) fn send(app: &str, message: &[u8]) -> Result<(), ShellError> {
    let sock = path(app, "sock");
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

pub(crate) fn event_source(_app: &str) {}

pub(crate) fn report_event(_id: crate::logging::EventId, _text: &str) {}

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
}
