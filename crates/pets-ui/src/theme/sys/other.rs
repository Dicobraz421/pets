//! Не Windows: ОС ничего не сообщает — светлая Fluent, акцент по умолчанию, политики нет, слежения нет.

use std::path::PathBuf;

use super::Signal;
use crate::theme::{OsLook, Policy};

pub(crate) fn os_look() -> OsLook {
    OsLook::default()
}

pub(crate) fn policy(_app: &str) -> Policy {
    Policy::default()
}

pub(crate) fn watch(_dir: Option<PathBuf>, _on: Box<dyn FnMut(Signal) -> bool + Send>) {}
