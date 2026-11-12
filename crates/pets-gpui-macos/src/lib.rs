//! Платформенный код окна приложений Pets на macOS за нейтральным API — аналог `gpui_windows::pets`
//! (pets-gpui-windows): приложение не держит objc2, CoreGraphics и `unsafe` для окна. Решение владельца 2026-10-28
//! «всё платформозависимое — в выделенные модули»; без UI (Связка ключей, экземпляр, LaunchServices) — в `pets-shell`.
//! Только Apple Silicon, macOS 13+ (Postman, решение 2026-11-11); на других ОС крейт пуст.
//!
//! `unsafe` — только здесь, у каждого блока — `SAFETY`. Ошибки — `std::io::Error`, без типов objc2 в сигнатурах;
//! окно — любое `HasWindowHandle` (`gpui::Window`) или его номер ([`window::id`]) для другого потока.
//!
//! - [`window`] — номер окна (`CGWindowID`) и снимок окна без тени (ScreenCaptureKit; macOS 13 — CoreGraphics);
//! - [`drag`] — перетаскивание файлов из окна в Finder и другие приложения (`NSDraggingSession`), копии во временном
//!   каталоге приложения;
//! - [`toast`] — уведомление центра уведомлений (`UNUserNotificationCenter`); нужен `.app` с идентификатором пакета;
//! - [`dock`] — бейдж на значке в Dock (`NSDockTile`), например число непрочитанных.

#[cfg(target_os = "macos")]
pub mod dock;
#[cfg(target_os = "macos")]
pub mod drag;
#[cfg(target_os = "macos")]
pub mod toast;
#[cfg(target_os = "macos")]
pub mod window;
