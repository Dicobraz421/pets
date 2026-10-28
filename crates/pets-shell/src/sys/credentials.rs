//! Секрет в диспетчере учётных данных Windows (CredWriteW / CredReadW / CredDeleteW): тип «общие учётные данные»,
//! хранение — для этого пользователя на этой машине. Имя записи (`target`) задаёт приложение. Секрет не попадает в
//! ошибки — только код Win32; копия в памяти при записи затирается.

use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
};

use super::{last_os, to_wide};
use crate::ShellError;

/// Записать (заменить) секрет `secret` пользователя `user` под именем `target`.
pub fn write(target: &str, user: &str, secret: &str) -> Result<(), ShellError> {
    let mut target = to_wide(target);
    let mut user = to_wide(user);
    let mut blob = secret.as_bytes().to_vec();
    let size = u32::try_from(blob.len()).map_err(|_| ShellError::Invalid {
        what: "CredWriteW",
        reason: "secret is too long",
    })?;
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        CredentialBlobSize: size,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: user.as_mut_ptr(),
        ..Default::default()
    };
    // SAFETY: строки и блоб живут до конца вызова; CredWriteW копирует их.
    let ok = unsafe { CredWriteW(&cred, 0) };
    let result = if ok == 0 { Err(last_os("CredWriteW")) } else { Ok(()) };
    // Копия секрета в памяти процесса — затереть.
    blob.fill(0);
    result
}

/// Секрет записи `target`; `Ok(None)` — такой записи нет.
pub fn read(target: &str) -> Result<Option<String>, ShellError> {
    let target = to_wide(target);
    let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: имя — строка с нулём на конце; результат освобождается CredFree ниже.
    let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) };
    if ok == 0 {
        // SAFETY: без аргументов, читает код ошибки потока.
        return if unsafe { GetLastError() } == ERROR_NOT_FOUND {
            Ok(None)
        } else {
            Err(last_os("CredReadW"))
        };
    }
    // SAFETY: при успехе `cred` указывает на запись, выделенную системой; блоб — `CredentialBlobSize` байт;
    // освобождается один раз.
    let secret = unsafe {
        let c = &*cred;
        let bytes = if c.CredentialBlob.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
        };
        let s = String::from_utf8(bytes.to_vec());
        CredFree(cred.cast());
        s
    };
    secret.map(Some).map_err(|_| ShellError::Invalid {
        what: "CredReadW",
        reason: "secret is not UTF-8",
    })
}

/// Удалить запись; нет записи — не ошибка.
pub fn delete(target: &str) -> Result<(), ShellError> {
    let target = to_wide(target);
    // SAFETY: имя — строка с нулём на конце.
    let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    // SAFETY: без аргументов, читает код ошибки потока.
    if ok == 0 && unsafe { GetLastError() } != ERROR_NOT_FOUND {
        Err(last_os("CredDeleteW"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// Пишет, читает и удаляет пробную запись в диспетчере учётных данных этого пользователя.
    #[test]
    #[ignore = "writes to the Windows Credential Manager of the current user"]
    fn credentials_round_trip() {
        let target = format!("Pets:test:{}", std::process::id());
        assert_eq!(super::read(&target).unwrap(), None);
        super::write(&target, "user", "dummy секрет").unwrap();
        assert_eq!(super::read(&target).unwrap().as_deref(), Some("dummy секрет"));
        super::write(&target, "user", "second").unwrap();
        assert_eq!(super::read(&target).unwrap().as_deref(), Some("second"));
        super::delete(&target).unwrap();
        assert_eq!(super::read(&target).unwrap(), None);
        super::delete(&target).unwrap();
    }

    /// Нет записи — `None` и удаление без ошибки (запись не создаётся).
    #[test]
    fn credentials_missing_is_none() {
        let target = format!(
            "Pets:test-missing:{}:{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        );
        assert_eq!(super::read(&target).unwrap(), None);
        super::delete(&target).unwrap();
    }
}
