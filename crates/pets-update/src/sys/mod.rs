//! Платформенный код: чтение политики из реестра. `unsafe` — только здесь.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::policy_dword;

/// Раздел реестра.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hive {
    /// `HKEY_LOCAL_MACHINE`.
    Machine,
    /// `HKEY_CURRENT_USER`.
    User,
}

/// Вне Windows политик нет.
#[cfg(not(windows))]
pub(crate) fn policy_dword(_hive: Hive, _path: &str, _name: &str) -> Option<u32> {
    None
}
