//! Платформенный код: единственный экземпляр, журнал событий ОС, реестр, учётные данные, стандартные потоки, разбивка
//! памяти, буфер обмена (HTML); macOS — экземпляр через unix-сокет, Связка ключей, обработчик схемы адресов, поток
//! ошибок в файл, буфер обмена (HTML), объект входа, управляемые настройки, память процесса. `unsafe` — только здесь.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;

#[cfg(not(any(windows, target_os = "macos")))]
mod other;
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) use other::*;

#[cfg(windows)]
pub(crate) mod clipboard;
#[cfg(windows)]
pub mod console;
#[cfg(windows)]
pub mod credentials;
#[cfg(windows)]
pub mod memory;
#[cfg(windows)]
pub mod registry;
#[cfg(windows)]
pub mod volume;

#[cfg(target_os = "macos")]
#[path = "clipboard_macos.rs"]
pub(crate) mod clipboard;
#[cfg(target_os = "macos")]
#[path = "console_macos.rs"]
pub mod console;
#[cfg(target_os = "macos")]
#[path = "credentials_macos.rs"]
pub mod credentials;
#[cfg(target_os = "macos")]
pub mod login_item;
#[cfg(target_os = "macos")]
#[path = "managed_macos.rs"]
pub mod managed;
#[cfg(target_os = "macos")]
#[path = "memory_macos.rs"]
pub mod memory;
#[cfg(target_os = "macos")]
pub mod url_scheme;

/// Строка UTF-16 с нулём в конце — для `PCWSTR`.
#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Ошибка ОС вызова `what`: `GetLastError` потока.
#[cfg(windows)]
fn last_os(what: &'static str) -> crate::ShellError {
    crate::ShellError::Os {
        what,
        source: std::io::Error::last_os_error(),
    }
}

/// Ошибка ОС вызова `what` по коду Win32.
#[cfg(windows)]
fn os_code(what: &'static str, code: u32) -> crate::ShellError {
    crate::ShellError::Os {
        what,
        source: std::io::Error::from_raw_os_error(code as i32),
    }
}
