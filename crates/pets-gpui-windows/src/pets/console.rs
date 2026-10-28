//! Стандартные потоки процесса без консоли (release, `windows_subsystem = "windows"`).
//!
//! У GUI-процесса stdout и stderr пусты, если их не перенаправили. [`attach_parent`] — для строк вывода в консоль
//! родителя: перенаправленный поток (`> файл`, канал Git Bash) остаётся как есть, пустой — в консоль родителя
//! (`AttachConsole(ATTACH_PARENT_PROCESS)`, затем `CONOUT$`); нет консоли у родителя (Проводник) — ничего.
//! [`stderr_to_file`] — журнал: поток ошибок — в файл.

use std::io;
use std::os::windows::io::IntoRawHandle as _;
use std::path::Path;

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Globalization::GetACP;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_UNKNOWN, GetFileType, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
};

use super::{last_error, wide};

/// Описатель, в который можно писать: файл, канал или консоль.
fn usable(h: HANDLE) -> bool {
    // SAFETY: GetFileType с чужим или закрытым описателем возвращает FILE_TYPE_UNKNOWN, а не UB.
    !h.is_null() && h != INVALID_HANDLE_VALUE && unsafe { GetFileType(h) } != FILE_TYPE_UNKNOWN
}

/// Пустые stdout и stderr — в консоль родителя, если она есть. Вызывать в начале `main`.
pub fn attach_parent() {
    const STREAMS: [STD_HANDLE; 2] = [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
    // SAFETY: GetStdHandle только читает таблицу стандартных описателей процесса.
    let before = STREAMS.map(|s| unsafe { GetStdHandle(s) });
    if before.iter().all(|&h| usable(h)) {
        return;
    }
    // SAFETY: без аргументов-указателей; неудача (нет консоли у родителя, уже есть своя) — просто 0.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    let conout = wide("CONOUT$");
    for (stream, old) in STREAMS.into_iter().zip(before) {
        if usable(old) {
            // Перенаправленный поток: AttachConsole мог заменить его консолью — вернуть.
            // SAFETY: возвращаем описатель, который был стандартным до вызова.
            unsafe { SetStdHandle(stream, old) };
            continue;
        }
        // SAFETY: только читает таблицу стандартных описателей.
        if usable(unsafe { GetStdHandle(stream) }) {
            continue;
        }
        // SAFETY: `conout` — строка UTF-16 с нулём в конце; атрибуты безопасности и шаблон не нужны.
        let h = unsafe {
            CreateFileW(
                conout.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE {
            // SAFETY: описатель консоли открыт выше и живёт до конца процесса.
            unsafe { SetStdHandle(stream, h) };
        }
    }
}

/// Процесс без консоли: поток ошибок — в конец файла `path` (туда пишет `tracing` в stderr). Поток ошибок уже есть
/// (консоль, перенаправление) — ничего, `Ok(false)`. `Ok(true)` — журнал пишется в файл.
pub fn stderr_to_file(path: &Path) -> io::Result<bool> {
    // SAFETY: без аргументов-указателей; описатель не закрываем.
    let current = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if !current.is_null() && current != INVALID_HANDLE_VALUE {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let handle = file.into_raw_handle();
    // SAFETY: описатель файла живёт до конца процесса (не закрывается); Rust читает стандартный поток при каждой
    // записи в stderr.
    if unsafe { SetStdHandle(STD_ERROR_HANDLE, handle) } == 0 {
        Err(last_error("SetStdHandle"))
    } else {
        Ok(true)
    }
}

/// Кодовая страница ANSI процесса: 65001, если действует манифест exe (`activeCodePage` UTF-8).
pub fn active_code_page() -> u32 {
    // SAFETY: без аргументов.
    unsafe { GetACP() }
}

#[cfg(test)]
mod tests {
    /// Тест идёт с потоками nextest/cargo: они есть — ни консоль, ни файл не подключаются.
    #[test]
    fn pets_console_keeps_existing_streams() {
        let path = std::env::temp_dir().join(format!("pets-console-test-{}.log", std::process::id()));
        assert!(!super::stderr_to_file(&path).unwrap());
        assert!(!path.exists());
        super::attach_parent();
        assert_ne!(super::active_code_page(), 0);
    }
}
