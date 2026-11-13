//! Платформенный код окна приложений Pets на macOS за нейтральным API — аналог `gpui_windows::pets`
//! (pets-gpui-windows): приложение не держит objc2, CoreGraphics и `unsafe` для окна. Решение владельца 2026-10-28
//! «всё платформозависимое — в выделенные модули»; без UI (Связка ключей, экземпляр, LaunchServices) — в `pets-shell`.
//! Только Apple Silicon, macOS 13+ (Postman, решение 2026-11-11); на других ОС крейт пуст.
//!
//! `unsafe` — только здесь, у каждого блока — `SAFETY`. Ошибки — `std::io::Error`, без типов objc2 в сигнатурах;
//! окно — любое `HasWindowHandle` (`gpui::Window`) или его номер ([`window::id`]) для другого потока.
//!
//! - [`window`] — номер окна (`CGWindowID`), снимок окна без тени (ScreenCaptureKit; macOS 13 — CoreGraphics) и
//!   просьба системе перерисовать окно (`redraw`: `setNeedsDisplay:` + `displayIfNeeded`);
//! - [`drag`] — перетаскивание файлов из окна в Finder и другие приложения (`NSDraggingSession`), копии во временном
//!   каталоге приложения;
//! - [`toast`] — уведомление центра уведомлений (`UNUserNotificationCenter`); нужен `.app` с идентификатором пакета;
//! - [`dock`] — бейдж на значке в Dock (`NSDockTile`), например число непрочитанных;
//! - [`status_item`] — значок в строке меню (`NSStatusItem`): шаблон, число рядом, меню приложения.

#[cfg(target_os = "macos")]
pub mod dock;
#[cfg(target_os = "macos")]
pub mod drag;
#[cfg(target_os = "macos")]
pub mod status_item;
#[cfg(target_os = "macos")]
pub mod toast;
#[cfg(target_os = "macos")]
pub mod window;

// GMAC-01: чистая логика диагностики кадров форка `pets-gpui-macos-platform` (кольцо событий, порог сторожа) —
// её тесты идут на любой ОС здесь: сам форк вне рабочей области и собирается только на macOS.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../pets-gpui-macos-platform/src/frames_logic.rs"]
mod frames_logic;
