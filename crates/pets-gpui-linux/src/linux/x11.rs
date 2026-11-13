mod client;
mod clipboard;
mod display;
mod event;
// Pets patch: источник XDND (перетаскивание файлов из окна).
mod pets_xdnd;
mod window;
mod xim_handler;

pub(crate) use client::*;
pub(crate) use clipboard::wait_for_clipboard_handovers;
pub(crate) use display::*;
pub(crate) use event::*;
#[cfg(test)]
pub(crate) use pets_xdnd::uri_list as pets_uri_list;
pub(crate) use window::*;
pub(crate) use xim_handler::*;
