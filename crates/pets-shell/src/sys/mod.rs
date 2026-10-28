//! Платформенный код: единственный экземпляр, журнал событий ОС, реестр, учётные данные, стандартные потоки, разбивка
//! памяти. `unsafe` — только здесь.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
pub(crate) use other::*;

#[cfg(windows)]
pub mod console;
#[cfg(windows)]
pub mod credentials;
#[cfg(windows)]
pub mod memory;
#[cfg(windows)]
pub mod registry;

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
