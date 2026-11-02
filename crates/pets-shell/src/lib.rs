//! Жизненный цикл процесса приложения Pets: журнал (tracing в stderr и журнал Windows), хук паники,
//! фатальная ошибка запуска, единственный экземпляр с передачей аргументов первому.
//!
//! Крейт без UI. Вывод окна первого экземпляра на передний план — в приложении (`on_args`); здесь —
//! только право на это (`AllowSetForegroundWindow`). Платформенный код — в `sys`.

mod logging;
mod message;
mod sys;

pub use logging::{fatal, init_logging};

/// Платформенный код приложений без UI (решение владельца 2026-10-28): реестр, учётные данные, стандартные потоки
/// процесса без консоли, разбивка памяти для бенча, шифрование тома BitLocker. Только Windows; `unsafe` — внутри
/// (`sys`).
#[cfg(windows)]
pub use sys::{console, credentials, memory, registry, volume};

/// Ошибка оболочки процесса.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("имя приложения «{0}»: нужно непустое, без `\\` и `/`")]
    BadName(String),
    #[error("{what}: {source}")]
    Os {
        what: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("первый экземпляр не принял аргументы за {0} мс")]
    NoServer(u32),
    #[error("сообщение {0} байт — больше предела {limit}", limit = message::MAX_LEN)]
    TooLarge(usize),
    #[error("аргумент содержит NUL")]
    Nul,
    #[error("сообщение не UTF-8")]
    NotUtf8,
    /// Данные не годятся для вызова ОС (длина, кодировка); сами данные (секрет) в ошибку не попадают.
    #[error("{what}: {reason}")]
    Invalid { what: &'static str, reason: &'static str },
}

/// Итог проверки единственного экземпляра.
#[derive(Debug)]
pub enum Instance {
    /// Этот процесс — первый: держит имя; аргументы следующих — через [`Listener::listen`].
    First(Listener),
    /// Уже запущен другой: аргументы переданы ему, приложение выходит с кодом 0.
    Second,
}

/// Владение именем экземпляра (mutex и канал). Пока жив — следующие запуски становятся `Second`.
#[derive(Debug)]
pub struct Listener {
    guard: sys::Guard,
}

impl Listener {
    /// Поток-сервер канала: на каждое подключение — `on_args(args)`. Ошибки — в журнал, поток живёт дальше.
    /// `on_args` вызывается в потоке сервера: в UI передавать через канал или исполнитель приложения.
    pub fn listen(self, on_args: impl Fn(Vec<String>) + Send + 'static) {
        let spawned = std::thread::Builder::new()
            .name("pets-shell-instance".into())
            .spawn(move || {
                sys::serve(self.guard, &|bytes: Vec<u8>| match message::decode(&bytes) {
                    Ok(args) => on_args(args),
                    Err(e) => tracing::warn!(target: "pets_shell", "аргументы второго экземпляра: {e}"),
                })
            });
        if let Err(e) = spawned {
            tracing::error!(target: "pets_shell", "поток канала экземпляра не запущен: {e}");
        }
    }
}

/// Единственный экземпляр: Windows — именованный mutex `Local\Pets.<app>` + named pipe
/// `\\.\pipe\Pets.<app>.<session id>`. Второй экземпляр передаёт свои аргументы первому и получает `Second`
/// (приложение выходит с кодом 0). Перед передачей — `AllowSetForegroundWindow(pid первого)`
/// (`GetNamedPipeServerProcessId`), чтобы первый смог вывести окно на передний план. Не Windows — всегда `First`.
///
/// Ошибка — приложение работает как первый экземпляр и пишет предупреждение. Относительные пути в `args`
/// второй экземпляр раскрывает сам: каталог у первого другой.
pub fn single_instance(app: &str, args: &[String]) -> Result<Instance, ShellError> {
    if app.is_empty() || app.contains(['\\', '/']) {
        return Err(ShellError::BadName(app.into()));
    }
    match sys::acquire(app)? {
        Some(guard) => Ok(Instance::First(Listener { guard })),
        None => {
            sys::send(app, &message::encode(args)?)?;
            Ok(Instance::Second)
        }
    }
}
