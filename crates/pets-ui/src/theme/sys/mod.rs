//! Платформенный код тем: вид окон ОС, политика, слежение за изменениями. `unsafe` — только здесь.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
pub(crate) use other::*;

/// Что изменилось (поток слежения).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Signal {
    /// Тема, акцент или контрастность ОС.
    Look,
    /// Файлы в папке тем пользователя.
    Dir,
}
