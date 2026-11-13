#![cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;
// Pets patch: платформенный код окна приложений Pets (`pets::drag`).
pub mod pets;

pub use linux::{current_platform, linux_platform};
