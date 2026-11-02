//! Шифрование тома BitLocker — свойством оболочки `System.Volume.BitLockerProtection` (как значок замка у диска в
//! Проводнике): без прав администратора, без WMI. Значение — число; смысл ([`protection`]) сверен с «Управлением
//! BitLocker»: 1 — защита включена, 2 — выключена, 3 — идёт шифрование, 4 — расшифровка, 5 — защита приостановлена,
//! 6 — включена (том заблокирован); нет значения — оболочка не знает (том без BitLocker-сведений, сетевой диск).

use std::ffi::c_void;
use std::path::Path;

use windows_sys::Win32::Foundation::PROPERTYKEY;
use windows_sys::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::System::Variant::{VT_I2, VT_I4, VT_INT, VT_UI1, VT_UI2, VT_UI4, VT_UINT};
use windows_sys::Win32::UI::Shell::PropertiesSystem::{
    GPS_DEFAULT, PSGetPropertyKeyFromName, SHGetPropertyStoreFromParsingName,
};
use windows_sys::core::{GUID, HRESULT};

use super::to_wide;
use crate::ShellError;

/// Шифрование тома.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitLocker {
    /// Защита включена (или идёт шифрование).
    On,
    /// Не зашифрован: выключено, расшифровывается или защита приостановлена.
    Off,
    /// Оболочка не сообщила.
    Unknown,
}

/// Значение свойства `System.Volume.BitLockerProtection` → состояние.
pub fn protection(value: Option<i64>) -> BitLocker {
    match value {
        Some(1 | 3 | 6) => BitLocker::On,
        Some(2 | 4 | 5) => BitLocker::Off,
        _ => BitLocker::Unknown,
    }
}

/// `IPropertyStore` — {886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99}.
const IID_IPROPERTYSTORE: GUID = GUID::from_u128(0x886d8eeb_8cf2_4446_8d02_cdba1dbdcf99);

/// Таблица методов `IPropertyStore` (IUnknown + GetCount, GetAt, GetValue).
#[repr(C)]
struct StoreVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    get_at: unsafe extern "system" fn(*mut c_void, u32, *mut PROPERTYKEY) -> HRESULT,
    get_value: unsafe extern "system" fn(*mut c_void, *const PROPERTYKEY, *mut PROPVARIANT) -> HRESULT,
}

fn hr(what: &'static str, code: HRESULT) -> ShellError {
    ShellError::Os {
        what,
        source: std::io::Error::from_raw_os_error(code),
    }
}

/// Шифрование тома, на котором лежит `path` (корень тома — `C:\`; путь внутри тома — его корень берётся сам).
pub fn bitlocker(path: &Path) -> Result<BitLocker, ShellError> {
    let root = path.ancestors().last().unwrap_or(path);
    let root = match root.to_str() {
        Some(r) if r.ends_with('\\') => r.to_owned(),
        Some(r) => format!("{r}\\"),
        None => {
            return Err(ShellError::Invalid {
                what: "bitlocker",
                reason: "path is not UTF-8",
            });
        }
    };
    // SAFETY: COM этого потока — на время вызова (свой CoInitializeEx — свой CoUninitialize); строки живут до конца
    // вызовов; указатель хранилища свойств освобождается Release; PROPVARIANT очищается PropVariantClear.
    unsafe {
        let co = CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32);
        let result = read(&root);
        if co >= 0 {
            CoUninitialize();
        }
        result
    }
}

/// Значение свойства тома `root` (COM потока уже есть).
unsafe fn read(root: &str) -> Result<BitLocker, ShellError> {
    let name = to_wide("System.Volume.BitLockerProtection");
    let mut key = PROPERTYKEY {
        fmtid: GUID::from_u128(0),
        pid: 0,
    };
    // SAFETY: см. `bitlocker`.
    unsafe {
        let code = PSGetPropertyKeyFromName(name.as_ptr(), &mut key);
        if code < 0 {
            return Err(hr("PSGetPropertyKeyFromName", code));
        }
        let path = to_wide(root);
        let mut store: *mut c_void = std::ptr::null_mut();
        let code = SHGetPropertyStoreFromParsingName(
            path.as_ptr(),
            std::ptr::null_mut(),
            GPS_DEFAULT,
            &IID_IPROPERTYSTORE,
            &mut store,
        );
        if code < 0 || store.is_null() {
            return Err(hr("SHGetPropertyStoreFromParsingName", code));
        }
        let vtbl = *(store as *const *const StoreVtbl);
        let mut pv = PROPVARIANT::default();
        let code = ((*vtbl).get_value)(store, &key, &mut pv);
        ((*vtbl).release)(store);
        if code < 0 {
            return Err(hr("IPropertyStore::GetValue", code));
        }
        let v = &pv.Anonymous.Anonymous;
        let value = match v.vt {
            VT_I4 | VT_INT => Some(i64::from(v.Anonymous.lVal)),
            VT_UI4 | VT_UINT => Some(i64::from(v.Anonymous.ulVal)),
            VT_I2 => Some(i64::from(v.Anonymous.iVal)),
            VT_UI2 => Some(i64::from(v.Anonymous.uiVal)),
            VT_UI1 => Some(i64::from(v.Anonymous.bVal)),
            _ => None,
        };
        PropVariantClear(&mut pv);
        Ok(protection(value))
    }
}

#[cfg(test)]
mod tests {
    use super::{BitLocker, protection};

    /// Значения свойства (приостановленная защита — «не зашифрован»), неизвестное — «неизвестно».
    #[test]
    fn values() {
        assert_eq!(protection(Some(1)), BitLocker::On);
        assert_eq!(protection(Some(3)), BitLocker::On);
        assert_eq!(protection(Some(6)), BitLocker::On);
        assert_eq!(protection(Some(2)), BitLocker::Off);
        assert_eq!(protection(Some(5)), BitLocker::Off);
        assert_eq!(protection(Some(4)), BitLocker::Off);
        assert_eq!(protection(Some(0)), BitLocker::Unknown);
        assert_eq!(protection(None), BitLocker::Unknown);
    }

    /// Системный том отвечает (значение — любое из трёх), путь внутри тома — его корень.
    #[test]
    fn system_volume_answers() {
        let windows = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into());
        assert!(super::bitlocker(&windows).is_ok());
    }
}
