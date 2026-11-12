//! Буфер обмена macOS: `public.html` (`NSPasteboardTypeHTML`) и `public.utf8-plain-text` (`NSPasteboardTypeString`).
//! В отличие от `CF_HTML` Windows, в буфере — сам документ HTML без заголовка смещений; разбор —
//! `crate::clipboard::parse_mac_html`. Здесь — только байты буфера.
//!
//! Каждая функция берёт буфер параметром: общий (`NSPasteboard::generalPasteboard`) — у приложения, свой
//! (`pasteboardWithUniqueName`) — у тестов, чтобы не трогать буфер пользователя. `NSPasteboard` — с любого потока.

use objc2::rc::Retained;
use objc2_app_kit::{NSPasteboard, NSPasteboardType, NSPasteboardTypeHTML, NSPasteboardTypeString};
use objc2_foundation::{NSArray, NSString};

use crate::ShellError;

/// Общий буфер обмена пользователя.
pub(crate) fn general() -> Retained<NSPasteboard> {
    NSPasteboard::generalPasteboard()
}

fn html_type() -> &'static NSPasteboardType {
    // SAFETY: неизменяемая константа AppKit, живёт весь процесс.
    unsafe { NSPasteboardTypeHTML }
}

fn text_type() -> &'static NSPasteboardType {
    // SAFETY: неизменяемая константа AppKit, живёт весь процесс.
    unsafe { NSPasteboardTypeString }
}

/// В буфере есть HTML.
pub(crate) fn has_html(pb: &NSPasteboard) -> bool {
    pb.availableTypeFromArray(&NSArray::from_slice(&[html_type()]))
        .is_some()
}

/// HTML из буфера (UTF-8) не больше `max` байт; нет или больше — `None`.
pub(crate) fn read_html(pb: &NSPasteboard, max: usize) -> Option<String> {
    let data = pb.dataForType(html_type())?;
    if data.length() > max {
        return None;
    }
    let bytes = data.to_vec();
    match String::from_utf8(bytes) {
        Ok(s) => Some(s),
        // Не UTF-8 (UTF-16 с BOM и т. п.): кодировку разберёт AppKit.
        Err(_) => pb.stringForType(html_type()).map(|s| s.to_string()),
    }
}

/// Положить HTML-документ и текст разом: прежнее содержимое буфера заменяется.
pub(crate) fn write(pb: &NSPasteboard, html: &str, text: &str) -> Result<(), ShellError> {
    pb.clearContents();
    let html_ok = pb.setString_forType(&NSString::from_str(html), html_type());
    let text_ok = pb.setString_forType(&NSString::from_str(text), text_type());
    if html_ok && text_ok {
        Ok(())
    } else {
        Err(ShellError::Os {
            what: "NSPasteboard setString:forType:",
            source: std::io::Error::other("pasteboard refused the data"),
        })
    }
}

#[cfg(test)]
mod tests {
    use objc2::msg_send;
    use objc2_app_kit::NSPasteboard;

    /// Свой буфер с уникальным именем: общий буфер пользователя не трогается. Записали HTML и текст — прочитали те
    /// же байты; больше предела — `None`.
    #[test]
    fn unique_pasteboard_round_trip() {
        let pb = NSPasteboard::pasteboardWithUniqueName();
        assert!(!super::has_html(&pb));
        super::write(&pb, "<b>Ж</b>", "Ж").unwrap();
        assert!(super::has_html(&pb));
        assert_eq!(super::read_html(&pb, 1 << 20).as_deref(), Some("<b>Ж</b>"));
        assert_eq!(super::read_html(&pb, 4), None, "больше предела — не читаем");
        // SAFETY: буфер с уникальным именем больше не нужен; `releaseGlobally` без аргументов.
        let () = unsafe { msg_send![&*pb, releaseGlobally] };
    }
}
