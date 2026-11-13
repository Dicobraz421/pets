//! Системный журнал macOS — единый журнал (os_log): подсистема — имя приложения, категории `error`, `warning`, `info`
//! (у os_log нет типа «предупреждение»: тип записи — `error`, `default`, `default`; паника — `fault`). Тип `info` не
//! берём: он не сохраняется на диск, а сведения на Windows хранятся (ADR 0011 — одно поведение). Просмотр: Console.app
//! или `log show --predicate 'subsystem == "<app>"'`. Вызов — `_os_log_impl` напрямую (макрос `os_log` — только в C):
//! формат `%{public}s`, буфер аргументов по раскладке `__builtin_os_log_format`.

use std::ffi::{CString, c_char, c_void};
use std::sync::OnceLock;

use crate::logging::EventId;

/// `os_log_type_t`.
const OS_LOG_TYPE_DEFAULT: u8 = 0x00;
const OS_LOG_TYPE_ERROR: u8 = 0x10;
const OS_LOG_TYPE_FAULT: u8 = 0x11;
/// Предел текста записи в байтах (как у журнала Windows); длиннее — обрезается по границе знака.
const MAX_TEXT: usize = 31_000;

unsafe extern "C" {
    static __dso_handle: u8;
    fn os_log_create(subsystem: *const c_char, category: *const c_char) -> *mut c_void;
    fn _os_log_impl(dso: *const c_void, log: *mut c_void, kind: u8, format: *const c_char, buf: *const u8, size: u32);
}

/// Объекты журнала по категориям: ошибки, предупреждения, сведения. Живут до конца процесса, потокобезопасны.
struct Logs([usize; 3]);

static LOGS: OnceLock<Option<Logs>> = OnceLock::new();

/// Открыть журнал подсистемы `app` (один раз за процесс).
pub(crate) fn event_source(app: &str) {
    LOGS.get_or_init(|| {
        let subsystem = CString::new(app).ok()?;
        let mut logs = [0usize; 3];
        for (slot, category) in logs.iter_mut().zip([c"error", c"warning", c"info"]) {
            // SAFETY: обе строки с NUL в конце; объект не освобождается (живёт до конца процесса).
            let log = unsafe { os_log_create(subsystem.as_ptr(), category.as_ptr()) };
            if log.is_null() {
                return None;
            }
            *slot = log as usize;
        }
        Some(Logs(logs))
    });
}

/// Записать событие в единый журнал; без [`event_source`] — ничего.
pub(crate) fn report_event(id: EventId, text: &str) {
    let Some(Some(logs)) = LOGS.get() else {
        return;
    };
    let (slot, kind) = match id {
        EventId::Crash => (0, OS_LOG_TYPE_FAULT),
        EventId::Error | EventId::StartupFailed => (0, OS_LOG_TYPE_ERROR),
        EventId::Warning => (1, OS_LOG_TYPE_DEFAULT),
        EventId::Info => (2, OS_LOG_TYPE_DEFAULT),
    };
    let mut end = text.len().min(MAX_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let Ok(line) = CString::new(text[..end].replace('\0', " ")) else {
        return;
    };
    let buf = args(line.as_ptr());
    // SAFETY: `__dso_handle` — заголовок этого образа (формат лежит в нём же); объект журнала создан
    // `os_log_create`; формат — строка с NUL; буфер — один аргумент-указатель на `line`, живой до возврата.
    unsafe {
        _os_log_impl(
            (&raw const __dso_handle).cast(),
            logs.0[slot] as *mut c_void,
            kind,
            c"%{public}s".as_ptr(),
            buf.as_ptr(),
            buf.len() as u32,
        );
    }
}

/// Буфер одного аргумента `%{public}s`: сводка (0x02 — есть не-скаляр), число аргументов, дескриптор (тип 2 — строка,
/// флаг 0x2 — public), размер 8, указатель.
fn args(s: *const c_char) -> [u8; 12] {
    let mut buf = [0u8; 12];
    buf[..4].copy_from_slice(&[0x02, 0x01, 0x22, 0x08]);
    buf[4..].copy_from_slice(&(s as u64).to_le_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Запись идёт в единый журнал без паники (проверка глазами — `log show --last 1m --predicate ...`).
    #[test]
    fn oslog_report_without_panic() {
        event_source("PetsShellTest");
        report_event(EventId::Info, "pets-shell test event");
        report_event(EventId::Error, "pets-shell test \0 error");
        assert_eq!(args(std::ptr::null())[..4], [0x02, 0x01, 0x22, 0x08]);
    }
}
