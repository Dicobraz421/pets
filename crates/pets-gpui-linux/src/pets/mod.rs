//! Pets patch: платформенный код окна приложений Pets на Linux за нейтральным API — аналог
//! `gpui_windows::pets` (pets-gpui-windows) и `pets_gpui_macos` (решение владельца 2026-10-28).
//!
//! - [`drag`] — перетаскивание файлов из окна в файловый менеджер и другие приложения: X11 — XDND
//!   (`linux/x11/pets_xdnd.rs`, наш патч), Wayland — `wl_data_device` (upstream); формат `text/uri-list`.

pub mod drag;
