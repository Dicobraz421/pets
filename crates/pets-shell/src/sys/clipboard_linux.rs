//! Буфер обмена Linux: тип `text/html` буфера `CLIPBOARD` — через `wl-paste` (Wayland, `wl-clipboard`) или `xclip`
//! (X11 и XWayland). Свой клиент протоколов не пишем: на Wayland чтение буфера без фокуса окна требует
//! `wlr-data-control`/`ext-data-control`, а окно и его фокус — у gpui. Утилиты нет — HTML в буфере «нет»
//! (предупреждение в журнал один раз). Как на macOS, в буфере — сам документ HTML (Firefox — UTF-16 с BOM);
//! разбор — `crate::clipboard::parse_mac_html`. Запись HTML на Linux не поддерживается.

use std::io::Read as _;
use std::process::{Command, Stdio};
use std::sync::Once;
use std::time::Duration;

/// Сколько ждать владельца буфера: занятый или зависший клиент не должен держать поток.
const TIMEOUT: Duration = Duration::from_secs(2);

const HTML: &str = "text/html";

/// Команды по сеансу: Wayland — `wl-paste`, иначе (или XWayland без `wl-paste`) — `xclip`.
fn tools() -> Vec<(&'static str, Vec<&'static str>, Vec<&'static str>)> {
    let mut out = Vec::new();
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        out.push(("wl-paste", vec!["--list-types"], vec!["--no-newline", "--type", HTML]));
    }
    if std::env::var_os("DISPLAY").is_some() {
        out.push((
            "xclip",
            vec!["-selection", "clipboard", "-target", "TARGETS", "-out"],
            vec!["-selection", "clipboard", "-target", HTML, "-out"],
        ));
    }
    out
}

static MISSING: Once = Once::new();

/// Вывод утилиты не больше `max` байт; утилиты нет, ошибка, больше предела или таймаут — `None`.
fn output(program: &'static str, args: &[&str], max: usize) -> Option<Vec<u8>> {
    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                MISSING.call_once(|| {
                    tracing::warn!(target: "pets_shell", "{program} not found: HTML clipboard is unavailable");
                });
            } else {
                tracing::debug!(target: "pets_shell", "{program}: {e}");
            }
            return None;
        }
    };
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let read = (&mut stdout).take(max as u64 + 1).read_to_end(&mut buf);
        let _ = tx.send(read.map(|_| buf));
    });
    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(buf)) if buf.len() <= max => match child.wait() {
            Ok(status) if status.success() => Some(buf),
            // Нет такого типа в буфере — утилита выходит с ошибкой: это не сбой.
            status => {
                tracing::debug!(target: "pets_shell", "{program}: {status:?}");
                None
            }
        },
        other => {
            match other {
                Ok(Ok(_)) => tracing::debug!(target: "pets_shell", "{program}: over {max} bytes"),
                Ok(Err(e)) => tracing::debug!(target: "pets_shell", "{program}: {e}"),
                Err(_) => tracing::debug!(target: "pets_shell", "{program}: timed out"),
            }
            // Больше предела или завис — процесс больше не нужен.
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

/// В буфере есть HTML.
pub(crate) fn has_html() -> bool {
    tools().into_iter().any(|(program, list, _)| {
        output(program, &list, 64 * 1024)
            .is_some_and(|types| String::from_utf8_lossy(&types).lines().any(|t| t.trim() == HTML))
    })
}

/// HTML из буфера не больше `max` байт; нет или больше — `None`.
pub(crate) fn read_html(max: usize) -> Option<String> {
    tools()
        .into_iter()
        .find_map(|(program, _, read)| output(program, &read, max))
        .and_then(|raw| decode(&raw))
}

/// UTF-8 (Chromium, LibreOffice) или UTF-16 с BOM (Firefox).
fn decode(raw: &[u8]) -> Option<String> {
    let utf16 = |be: bool| {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| {
                if be {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16(&units).ok()
    };
    match raw {
        [0xFF, 0xFE, ..] => utf16(false),
        [0xFE, 0xFF, ..] => utf16(true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8(rest.to_vec()).ok(),
        _ => String::from_utf8(raw.to_vec()).ok(),
    }
}

#[cfg(test)]
mod tests {
    /// UTF-8, UTF-8 с BOM и UTF-16 LE с BOM дают одну строку.
    #[test]
    fn clipboard_html_decode() {
        let s = "<b>Ж</b>";
        assert_eq!(super::decode(s.as_bytes()).as_deref(), Some(s));
        let bom8 = [&[0xEF, 0xBB, 0xBF][..], s.as_bytes()].concat();
        assert_eq!(super::decode(&bom8).as_deref(), Some(s));
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(s.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(super::decode(&le).as_deref(), Some(s));
    }
}
