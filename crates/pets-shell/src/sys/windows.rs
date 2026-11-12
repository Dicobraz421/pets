//! Windows: именованный mutex `Local\Pets.<app>`, канал `\\.\pipe\Pets.<app>.<сеанс>`, журнал событий.

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA,
    ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_READ_ATTRIBUTES, OPEN_EXISTING, PIPE_ACCESS_INBOUND, ReadFile,
    SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::EventLog::{
    EVENTLOG_ERROR_TYPE, EVENTLOG_INFORMATION_TYPE, EVENTLOG_WARNING_TYPE, RegisterEventSourceW, ReportEventW,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId, PIPE_READMODE_MESSAGE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

use crate::ShellError;
use crate::logging::{EventId, Severity};
use crate::message::MAX_LEN;

/// Сколько второй экземпляр ждёт канал первого (тот мог взять mutex и ещё не создать канал).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Пауза между попытками, пока канала нет.
const RETRY: Duration = Duration::from_millis(50);
/// Предел строки события в UTF-16 (у ReportEventW — 31 839).
const EVENT_MAX_UTF16: usize = 31_000;

/// Описатель ядра; закрывается при сбросе.
#[derive(Debug)]
struct Handle(HANDLE);

// Описатель ядра — число в таблице процесса, им можно пользоваться из любого потока.
unsafe impl Send for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: описатель получен от ОС, принадлежит нам и закрывается один раз.
        unsafe { CloseHandle(self.0) };
    }
}

/// Владение именем: mutex и экземпляр канала (один, переиспользуется — не закрывать, иначе имя займут).
#[derive(Debug)]
pub(crate) struct Guard {
    _mutex: Handle,
    pipe: Handle,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn os(what: &'static str, code: u32) -> ShellError {
    ShellError::Os {
        what,
        source: std::io::Error::from_raw_os_error(code as i32),
    }
}

fn last_error() -> u32 {
    // SAFETY: без аргументов, читает код ошибки потока.
    unsafe { GetLastError() }
}

/// Имя канала: пространство каналов общее для всех сеансов — номер сеанса разводит пользователей RDP/быстрой смены.
fn pipe_name(app: &str) -> Result<Vec<u16>, ShellError> {
    let mut session = 0u32;
    // SAFETY: указатель на локальную переменную.
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0 {
        return Err(os("ProcessIdToSessionId", last_error()));
    }
    Ok(wide(&format!(r"\\.\pipe\Pets.{app}.{session}")))
}

/// Занять имя: `Some` — первый экземпляр (mutex и канал созданы), `None` — имя уже занято.
pub(crate) fn acquire(app: &str) -> Result<Option<Guard>, ShellError> {
    let name = wide(&format!(r"Local\Pets.{app}"));
    // SAFETY: имя — строка с NUL, атрибуты по умолчанию.
    let raw = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
    let err = last_error();
    if raw.is_null() {
        // Mutex есть, но создан с другими правами (напр. процессом администратора) — тоже «уже запущен».
        return if err == ERROR_ACCESS_DENIED {
            Ok(None)
        } else {
            Err(os("CreateMutexW", err))
        };
    }
    let mutex = Handle(raw);
    if err == ERROR_ALREADY_EXISTS {
        return Ok(None);
    }
    let pipe_name = pipe_name(app)?;
    // FIRST_PIPE_INSTANCE: если имя уже занял кто-то другой — отказ, а не подключение к чужому серверу.
    // SAFETY: имя — строка с NUL, атрибуты по умолчанию (запись — только владельцу и администраторам).
    let pipe = unsafe {
        CreateNamedPipeW(
            pipe_name.as_ptr(),
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            MAX_LEN as u32,
            0,
            null(),
        )
    };
    if pipe == INVALID_HANDLE_VALUE {
        return Err(os("CreateNamedPipeW", last_error()));
    }
    Ok(Some(Guard {
        _mutex: mutex,
        pipe: Handle(pipe),
    }))
}

/// Подключиться к каналу первого экземпляра (повторы до `CONNECT_TIMEOUT`) и передать сообщение.
pub(crate) fn send(app: &str, message: &[u8]) -> Result<(), ShellError> {
    let name = pipe_name(app)?;
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let pipe = loop {
        // SQOS_PRESENT | IDENTIFICATION: сервер не сможет действовать от имени клиента.
        // SAFETY: имя — строка с NUL, остальные аргументы — значения.
        let raw = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_WRITE | FILE_READ_ATTRIBUTES,
                0,
                null(),
                OPEN_EXISTING,
                SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                null_mut(),
            )
        };
        if raw != INVALID_HANDLE_VALUE {
            break Handle(raw);
        }
        let err = last_error();
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ShellError::NoServer(CONNECT_TIMEOUT.as_millis() as u32));
        }
        match err {
            // Канал есть, но занят другим клиентом — ждём освобождения.
            // SAFETY: имя — строка с NUL.
            ERROR_PIPE_BUSY => unsafe {
                WaitNamedPipeW(name.as_ptr(), left.as_millis().clamp(1, u32::MAX as u128) as u32);
            },
            // Канала ещё нет: первый взял mutex, но не дошёл до CreateNamedPipeW.
            ERROR_FILE_NOT_FOUND => std::thread::sleep(RETRY.min(left)),
            _ => return Err(os("CreateFileW", err)),
        }
    };
    let mut server = 0u32;
    // SAFETY: описатель открыт, указатель на локальную переменную.
    if unsafe { GetNamedPipeServerProcessId(pipe.0, &mut server) } != 0 {
        // Право вывести окно на передний план есть у нас (запуск пользователем) — отдаём его первому.
        // SAFETY: только число.
        if unsafe { AllowSetForegroundWindow(server) } == 0 {
            tracing::debug!(target: "pets_shell", "AllowSetForegroundWindow({server}): {}", last_error());
        }
    } else {
        tracing::debug!(target: "pets_shell", "GetNamedPipeServerProcessId: {}", last_error());
    }
    let mut written = 0u32;
    // SAFETY: буфер жив до возврата, длина ≤ MAX_LEN, синхронная запись.
    let ok = unsafe { WriteFile(pipe.0, message.as_ptr(), message.len() as u32, &mut written, null_mut()) };
    if ok == 0 {
        return Err(os("WriteFile", last_error()));
    }
    Ok(())
}

/// Цикл сервера: подключение → сообщение целиком → отключение → `on_message`. Тот же экземпляр канала
/// переиспользуется. Не возвращается.
pub(crate) fn serve(guard: Guard, on_message: &dyn Fn(Vec<u8>)) {
    let pipe = guard.pipe.0;
    loop {
        // SAFETY: описатель канала жив, пока жив `guard`; синхронный вызов.
        if unsafe { ConnectNamedPipe(pipe, null_mut()) } == 0 {
            match last_error() {
                // Клиент подключился раньше вызова; NO_DATA — уже и закрылся, данные в буфере остаются.
                ERROR_PIPE_CONNECTED | ERROR_NO_DATA => {}
                err => {
                    tracing::error!(target: "pets_shell", "ConnectNamedPipe: {}", std::io::Error::from_raw_os_error(err as i32));
                    disconnect(pipe);
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
            }
        }
        let read = read_message(pipe);
        disconnect(pipe);
        match read {
            Ok(bytes) => on_message(bytes),
            Err(e) => tracing::warn!(target: "pets_shell", "сообщение второго экземпляра: {e}"),
        }
    }
}

fn disconnect(pipe: HANDLE) {
    // SAFETY: описатель канала жив.
    unsafe { DisconnectNamedPipe(pipe) };
}

/// Прочитать одно сообщение целиком (повторы на ERROR_MORE_DATA), не больше `MAX_LEN`.
fn read_message(pipe: HANDLE) -> Result<Vec<u8>, ShellError> {
    let mut message = Vec::new();
    let mut chunk = vec![0u8; 16 * 1024];
    loop {
        let mut n = 0u32;
        // SAFETY: буфер жив до возврата, длина — его размер; синхронное чтение.
        let ok = unsafe { ReadFile(pipe, chunk.as_mut_ptr(), chunk.len() as u32, &mut n, null_mut()) } != 0;
        let err = if ok { 0 } else { last_error() };
        message.extend_from_slice(&chunk[..n as usize]);
        if message.len() > MAX_LEN {
            return Err(ShellError::TooLarge(message.len()));
        }
        match err {
            0 => return Ok(message),
            ERROR_MORE_DATA => {}
            // Клиент закрылся, ничего не прислав (или прислал пустое сообщение) — запуск без аргументов.
            ERROR_BROKEN_PIPE if message.is_empty() => return Ok(message),
            err => return Err(os("ReadFile", err)),
        }
    }
}

/// Источник журнала событий; описатель потокобезопасен.
struct Source(HANDLE);

// RegisterEventSourceW: описатель можно использовать из любого потока.
unsafe impl Send for Source {}
unsafe impl Sync for Source {}

static SOURCE: OnceLock<Option<Source>> = OnceLock::new();

/// Открыть источник `app` в журнале «Приложение» (один раз за процесс). Источник не зарегистрирован в
/// реестре (это делает установщик) — запись всё равно идёт, просмотр событий пишет «описание не найдено».
pub(crate) fn event_source(app: &str) {
    SOURCE.get_or_init(|| {
        let name = wide(app);
        // SAFETY: локальный компьютер, имя — строка с NUL.
        let raw = unsafe { RegisterEventSourceW(null(), name.as_ptr()) };
        (!raw.is_null()).then_some(Source(raw))
    });
}

/// Записать событие одной строкой в журнал Windows: код — [`EventId`], тип — по его важности. Без источника — ничего.
pub(crate) fn report_event(id: EventId, text: &str) {
    let Some(Some(source)) = SOURCE.get() else {
        return;
    };
    let mut line: Vec<u16> = Vec::with_capacity(text.len().min(EVENT_MAX_UTF16) + 1);
    for c in text.chars() {
        if line.len() + c.len_utf16() > EVENT_MAX_UTF16 {
            break;
        }
        let mut buf = [0u16; 2];
        line.extend_from_slice(c.encode_utf16(&mut buf));
    }
    line.push(0);
    let strings = [line.as_ptr()];
    // SAFETY: источник открыт, одна строка с NUL жива до возврата, без SID и двоичных данных.
    unsafe {
        ReportEventW(
            source.0,
            match id.severity() {
                Severity::Error => EVENTLOG_ERROR_TYPE,
                Severity::Warning => EVENTLOG_WARNING_TYPE,
                Severity::Info => EVENTLOG_INFORMATION_TYPE,
            },
            0,
            id as u32,
            null_mut(),
            1,
            0,
            strings.as_ptr(),
            null::<c_void>(),
        )
    };
}
