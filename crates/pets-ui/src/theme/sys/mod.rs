//! Платформенный код тем: вид окон ОС, политика, слежение за изменениями. `unsafe` — только здесь.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::*;

#[cfg(not(any(windows, target_os = "macos")))]
mod other;
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) use other::*;

/// Что изменилось (поток слежения).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Signal {
    /// Тема, акцент или контрастность ОС.
    Look,
    /// Файлы в папке тем пользователя.
    Dir,
}
