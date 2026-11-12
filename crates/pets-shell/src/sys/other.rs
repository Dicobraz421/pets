//! Не Windows: экземпляр всегда первый, журнала событий ОС нет.

use crate::ShellError;

/// Владения нет: имя не занимается.
#[derive(Debug)]
pub(crate) struct Guard;

pub(crate) fn acquire(_app: &str) -> Result<Option<Guard>, ShellError> {
    Ok(Some(Guard))
}

pub(crate) fn send(_app: &str, _message: &[u8]) -> Result<(), ShellError> {
    Ok(())
}

/// Сервера нет: подключений не будет.
pub(crate) fn serve(_guard: Guard, _on_message: &dyn Fn(Vec<u8>)) {}

pub(crate) fn event_source(_app: &str) {}

pub(crate) fn report_event(_id: crate::logging::EventId, _text: &str) {}
