//! Буфер обмена Windows: `CF_HTML` («HTML Format», регистрируемый формат) и `CF_UNICODETEXT`. Разбор и сборка
//! `CF_HTML` — `crate::clipboard`; здесь — только байты буфера.

use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};

use super::{last_os, to_wide};
use crate::ShellError;

/// `CF_UNICODETEXT` (стандартный формат 13; константа — в `Win32_System_Ole`, его ради неё не подключаем).
const CF_UNICODETEXT: u32 = 13;

/// Сколько раз пробовать открыть буфер: его держит другой процесс доли миллисекунды.
const OPEN_TRIES: u32 = 5;

/// Номер формата «HTML Format»; `None` — регистрация не удалась.
fn html_format() -> Option<u32> {
    let name = to_wide("HTML Format");
    // SAFETY: строка с нулём на конце живёт до конца вызова.
    let f = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
    (f != 0).then_some(f)
}

/// Буфер открыт этим потоком; закрывается при выходе из области.
struct Open;

impl Open {
    fn new() -> Result<Open, ShellError> {
        for _ in 0..OPEN_TRIES {
            // SAFETY: без окна-владельца; закрывает `Drop`.
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return Ok(Open);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        Err(last_os("OpenClipboard"))
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: буфер открыт этим потоком (`Open::new`).
        unsafe { CloseClipboard() };
    }
}

pub(crate) fn has_html() -> bool {
    // SAFETY: вызов без указателей.
    html_format().is_some_and(|f| unsafe { IsClipboardFormatAvailable(f) } != 0)
}

/// Байты `CF_HTML` не больше `max`; нет HTML или больше — `None`.
pub(crate) fn read_html(max: usize) -> Result<Option<Vec<u8>>, ShellError> {
    let Some(format) = html_format() else { return Ok(None) };
    if !has_html() {
        return Ok(None);
    }
    let _open = Open::new()?;
    // SAFETY: буфер открыт; дескриптор принадлежит буферу — не освобождаем.
    let mem = unsafe { GetClipboardData(format) };
    if mem.is_null() {
        return Ok(None);
    }
    // SAFETY: `mem` — глобальная память буфера, открытого этим потоком.
    let size = unsafe { GlobalSize(mem) };
    if size == 0 || size > max {
        return Ok(None);
    }
    // SAFETY: см. выше; указатель действителен до `GlobalUnlock`.
    let ptr = unsafe { GlobalLock(mem) }.cast::<u8>();
    if ptr.is_null() {
        return Err(last_os("GlobalLock"));
    }
    // SAFETY: `ptr` указывает на `size` байт, пока память заблокирована; копируем и сразу отпускаем.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, size) }.to_vec();
    // SAFETY: снимаем свою блокировку.
    unsafe { GlobalUnlock(mem) };
    Ok(Some(bytes))
}

/// Глобальная память с копией `bytes` — для `SetClipboardData` (после успеха ею владеет буфер).
fn global(bytes: &[u8]) -> Result<HANDLE, ShellError> {
    // SAFETY: выделение без указателей; при ошибке — null.
    let mem = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) };
    if mem.is_null() {
        return Err(last_os("GlobalAlloc"));
    }
    // SAFETY: `mem` — только что выделенная память не меньше `bytes.len()` байт.
    let ptr = unsafe { GlobalLock(mem) }.cast::<u8>();
    if ptr.is_null() {
        let e = last_os("GlobalLock");
        // SAFETY: память наша, буферу не передана.
        unsafe { GlobalFree(mem) };
        return Err(e);
    }
    // SAFETY: области не пересекаются, размер — не больше выделенного.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        GlobalUnlock(mem);
    }
    Ok(mem)
}

/// Отдать память буферу; не взял — освободить самим.
fn set(format: u32, bytes: &[u8]) -> Result<(), ShellError> {
    let mem = global(bytes)?;
    // SAFETY: буфер открыт и очищен этим потоком; при успехе памятью владеет система.
    if unsafe { SetClipboardData(format, mem) }.is_null() {
        let e = last_os("SetClipboardData");
        // SAFETY: буфер память не взял — она наша.
        unsafe { GlobalFree(mem) };
        return Err(e);
    }
    Ok(())
}

/// `CF_HTML` (уже собранный, без нуля в конце) и текст — в буфер разом.
pub(crate) fn write(cf_html: &[u8], text: &str) -> Result<(), ShellError> {
    let format = html_format().ok_or_else(|| last_os("RegisterClipboardFormatW"))?;
    let _open = Open::new()?;
    // SAFETY: буфер открыт этим потоком.
    if unsafe { EmptyClipboard() } == 0 {
        return Err(last_os("EmptyClipboard"));
    }
    let mut html = cf_html.to_vec();
    html.push(0);
    set(format, &html)?;
    let wide: Vec<u8> = to_wide(text).iter().flat_map(|u| u.to_le_bytes()).collect();
    set(CF_UNICODETEXT, &wide)
}
