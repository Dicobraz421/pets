//! Pets patch: платформенный код приложений Pets за нейтральным API — приложение не держит `windows`/`windows-sys` и
//! `unsafe` для окна и оболочки ОС. Решение владельца 2026-10-28: «всё платформозависимое — в выделенные модули».
//!
//! `unsafe` — только в этих модулях, у каждого блока — `SAFETY`. Ошибки — `std::io::Error` (код и текст ОС), без
//! типов `windows` в сигнатурах; окно — `gpui::Window` или его номер ([`window::id`]) для другого потока.
//!
//! - [`drag`] — перетаскивание файлов из окна в другие приложения (OLE drag source): имя и байты или ленивый
//!   источник, запасная копия на диске во временном каталоге приложения;
//! - [`window`] — значки окна из ресурса exe, снимок клиентской области;
//! - [`toast`] — уведомление центра уведомлений от имени приложения без ярлыка в «Пуске»;
//! - [`registry`] — строковые значения реестра (раздел пользователя, чтение — и машины);
//! - [`credentials`] — секреты в диспетчере учётных данных Windows;
//! - [`console`] — консоль родителя и поток ошибок в файл для процесса без консоли;
//! - [`memory`] — разбивка памяти процесса для бенча.

pub mod console;
pub mod credentials;
pub mod drag;
pub mod memory;
pub mod registry;
pub mod toast;
pub mod window;

/// Строка UTF-16 с нулём в конце — для `PCWSTR`.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Ошибка ОС с именем вызова: `GetLastError` потока.
fn last_error(what: &str) -> std::io::Error {
    let e = std::io::Error::last_os_error();
    std::io::Error::new(e.kind(), format!("{what}: {e}"))
}

/// Ошибка `windows` (HRESULT) — в `io::Error` с именем вызова.
fn com_error(what: &str, e: windows::core::Error) -> std::io::Error {
    std::io::Error::other(format!("{what}: {}", e.message()))
}
