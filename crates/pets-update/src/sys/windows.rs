//! Реестр Windows: значение `REG_DWORD` политики.

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW};

use super::Hive;

/// Строка UTF-16 с нулём в конце — для `PCWSTR`.
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Значение `REG_DWORD`; нет ключа, значения или другой тип — `None`.
pub(crate) fn policy_dword(hive: Hive, path: &str, name: &str) -> Option<u32> {
    let root = match hive {
        Hive::Machine => HKEY_LOCAL_MACHINE,
        Hive::User => HKEY_CURRENT_USER,
    };
    let (wpath, wname) = (to_wide(path), to_wide(name));
    let mut value = 0u32;
    let mut size = 4u32;
    // SAFETY: строки UTF-16 с нулём живут до конца вызова; буфер — `u32`, его размер в байтах передан в `size`.
    let rc = unsafe {
        RegGetValueW(
            root,
            wpath.as_ptr(),
            wname.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut value).cast(),
            &raw mut size,
        )
    };
    (rc == ERROR_SUCCESS).then_some(value)
}
