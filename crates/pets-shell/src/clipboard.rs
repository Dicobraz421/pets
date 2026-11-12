//! HTML в буфере обмена (`CF_HTML`, «HTML Format»): то, чего нет у буфера обмена gpui (там — текст, картинки, файлы).
//! Вставка из Word и браузера с таблицами и разметкой, копирование с форматированием в Word и Outlook.
//!
//! Формат — заголовок `Version:0.9`, смещения `StartHTML`/`EndHTML`/`StartFragment`/`EndFragment` (байты от начала
//! буфера, UTF-8), необязательный `SourceURL`, затем документ с `<!--StartFragment-->…<!--EndFragment-->`. Разбор и
//! сборка — здесь и без ОС ([`parse_cf_html`], [`encode_cf_html`]); чтение и запись — Windows (`sys::clipboard`),
//! на других ОС HTML в буфере нет.
//!
//! HTML буфера — чужие данные: приложение разбирает его как письмо (в песочнице), здесь — только байты фрагмента.

use crate::ShellError;

/// Больше HTML из буфера не читаем по умолчанию: вставка — не загрузка документа.
pub const DEFAULT_MAX: usize = 16 << 20;

/// HTML из буфера обмена.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Html {
    /// Выделенное в источнике (между `StartFragment` и `EndFragment`); смещений нет — весь документ.
    pub fragment: String,
    /// Откуда скопировано (`SourceURL` браузера); Word его не пишет.
    pub source_url: Option<String>,
}

/// Разобрать `CF_HTML`. Смещения вне буфера или не по порядку — берётся документ после заголовка (`StartHTML`,
/// иначе с первого `<`); пустой фрагмент — `None`. Нуль в конце (так пишет Windows) отбрасывается.
pub fn parse_cf_html(raw: &[u8]) -> Option<Html> {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let raw = &raw[..end];
    let head_end = raw.iter().position(|&b| b == b'<').unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..head_end]);
    let field = |name: &str| -> Option<&str> {
        head.lines()
            .find_map(|l| l.trim().strip_prefix(name)?.strip_prefix(':'))
    };
    let offset = |name: &str| field(name).and_then(|v| v.trim().parse::<usize>().ok());
    let slice = match (offset("StartFragment"), offset("EndFragment")) {
        (Some(a), Some(b)) if a < b && b <= raw.len() => &raw[a..b],
        _ => match offset("StartHTML").filter(|&a| a < raw.len()) {
            Some(a) => &raw[a..],
            None => &raw[head_end..],
        },
    };
    let fragment = String::from_utf8_lossy(slice).trim().to_owned();
    let source_url = field("SourceURL")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    (!fragment.is_empty()).then_some(Html { fragment, source_url })
}

/// Собрать `CF_HTML` из фрагмента: документ `<html><body><!--StartFragment-->…<!--EndFragment--></body></html>`,
/// смещения — десятизначные (длина заголовка не зависит от чисел). Переводы строк в `source_url` убираются.
pub fn encode_cf_html(fragment: &str, source_url: Option<&str>) -> Vec<u8> {
    const PRE: &str = "<html><body>\r\n<!--StartFragment-->";
    const POST: &str = "<!--EndFragment-->\r\n</body></html>";
    let url = source_url.map(|u| u.replace(['\r', '\n'], ""));
    let head = |a: usize, b: usize, c: usize, d: usize| {
        let mut h = format!(
            "Version:0.9\r\nStartHTML:{a:010}\r\nEndHTML:{b:010}\r\nStartFragment:{c:010}\r\nEndFragment:{d:010}\r\n"
        );
        if let Some(u) = &url {
            h.push_str("SourceURL:");
            h.push_str(u);
            h.push_str("\r\n");
        }
        h
    };
    let len = head(0, 0, 0, 0).len();
    let start_fragment = len + PRE.len();
    let end_fragment = start_fragment + fragment.len();
    let end_html = end_fragment + POST.len();
    let mut out = head(len, end_html, start_fragment, end_fragment).into_bytes();
    out.extend_from_slice(PRE.as_bytes());
    out.extend_from_slice(fragment.as_bytes());
    out.extend_from_slice(POST.as_bytes());
    out
}

/// Разобрать `public.html` macOS: между `<!--StartFragment-->` и `<!--EndFragment-->` (Word), иначе содержимое
/// `<body>`, иначе весь документ; ведущие `<meta …>` (Safari пишет `<meta charset='utf-8'>`) отбрасываются. Пустой —
/// `None`. Адреса источника в буфере macOS нет.
pub fn parse_mac_html(raw: &str) -> Option<Html> {
    let lower = raw.to_ascii_lowercase();
    let between = |open: &str, close: &str| -> Option<&str> {
        let a = lower.find(open)? + open.len();
        let b = a + lower[a..].find(close)?;
        Some(&raw[a..b])
    };
    let body = || -> Option<&str> {
        let tag = lower.find("<body")?;
        let a = tag + lower[tag..].find('>')? + 1;
        let b = lower.rfind("</body>").filter(|&b| b >= a).unwrap_or(raw.len());
        Some(&raw[a..b])
    };
    let mut fragment = between("<!--startfragment-->", "<!--endfragment-->")
        .or_else(body)
        .unwrap_or(raw)
        .trim();
    while fragment.get(..5).is_some_and(|h| h.eq_ignore_ascii_case("<meta")) {
        match fragment.find('>') {
            Some(end) => fragment = fragment[end + 1..].trim_start(),
            None => break,
        }
    }
    let fragment = fragment.trim();
    (!fragment.is_empty()).then(|| Html {
        fragment: fragment.to_owned(),
        source_url: None,
    })
}

/// Собрать `public.html` macOS из фрагмента: документ UTF-8 с `<!--StartFragment-->…<!--EndFragment-->` (так его
/// узнают Word и Pages).
pub fn encode_mac_html(fragment: &str) -> String {
    format!(
        "<html><head><meta charset=\"utf-8\"></head><body><!--StartFragment-->{fragment}<!--EndFragment--></body></html>"
    )
}

/// В буфере обмена есть HTML (кнопка «Вставить с форматированием», меню).
pub fn has_html() -> bool {
    #[cfg(windows)]
    {
        crate::sys::clipboard::has_html()
    }
    #[cfg(target_os = "macos")]
    {
        crate::sys::clipboard::has_html(&crate::sys::clipboard::general())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

/// HTML из буфера обмена не больше `max` байт (с заголовком); нет HTML или он больше — `Ok(None)`. Буфер занят другим
/// процессом — ошибка ОС.
pub fn read_html(max: usize) -> Result<Option<Html>, ShellError> {
    #[cfg(windows)]
    {
        Ok(crate::sys::clipboard::read_html(max)?.and_then(|raw| parse_cf_html(&raw)))
    }
    #[cfg(target_os = "macos")]
    {
        let pb = crate::sys::clipboard::general();
        Ok(crate::sys::clipboard::read_html(&pb, max).and_then(|raw| parse_mac_html(&raw)))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = max;
        Ok(None)
    }
}

/// Положить в буфер обмена HTML-фрагмент и его текст (`CF_HTML` и `CF_UNICODETEXT` разом; macOS — `public.html` и
/// `public.utf8-plain-text`): Word и Outlook вставят с форматированием, Блокнот и TextEdit — текст. Прежнее
/// содержимое буфера заменяется.
pub fn write_html(fragment: &str, text: &str, source_url: Option<&str>) -> Result<(), ShellError> {
    #[cfg(windows)]
    {
        crate::sys::clipboard::write(&encode_cf_html(fragment, source_url), text)
    }
    #[cfg(target_os = "macos")]
    {
        // Поля адреса источника у `public.html` нет.
        let _ = source_url;
        let pb = crate::sys::clipboard::general();
        crate::sys::clipboard::write(&pb, &encode_mac_html(fragment), text)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (fragment, text, source_url);
        Err(ShellError::Invalid {
            what: "clipboard",
            reason: "HTML clipboard is Windows and macOS only",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Сборка и разбор — туда и обратно, смещения указывают на фрагмент (UTF-8, кириллица).
    #[test]
    fn encode_then_parse() {
        let frag = "<table><tr><td><b>Город</b></td></tr></table>";
        let raw = encode_cf_html(frag, Some("https://corp.example/r"));
        let text = String::from_utf8(raw.clone()).unwrap();
        let at = |name: &str| -> usize {
            let l = text.lines().find(|l| l.starts_with(name)).unwrap();
            l[name.len() + 1..].parse().unwrap()
        };
        assert_eq!(&raw[at("StartFragment")..at("EndFragment")], frag.as_bytes());
        assert_eq!(at("EndHTML"), raw.len());
        assert!(text[at("StartHTML")..].starts_with("<html>"));
        let back = parse_cf_html(&raw).unwrap();
        assert_eq!(
            back,
            Html {
                fragment: frag.into(),
                source_url: Some("https://corp.example/r".into())
            }
        );
    }

    /// Как пишет Word: смещения, документ со стилями, нуль в конце; без смещений — документ после заголовка.
    #[test]
    fn parse_word_and_broken_headers() {
        let body =
            "<html><head><style>p{}</style></head><body><!--StartFragment--><p>Ж</p><!--EndFragment--></body></html>";
        let head_len = encode_cf_html("", None).iter().position(|&b| b == b'<').unwrap();
        let start = head_len + body.find("<p>").unwrap();
        let end = head_len + body.find("<!--EndFragment").unwrap();
        let raw = format!(
            "Version:0.9\r\nStartHTML:{head_len:010}\r\nEndHTML:{:010}\r\nStartFragment:{start:010}\r\nEndFragment:{end:010}\r\n{body}\0",
            head_len + body.len()
        );
        assert_eq!(head_len, "Version:0.9\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n".len());
        let h = parse_cf_html(raw.as_bytes()).unwrap();
        assert_eq!((h.fragment.as_str(), h.source_url), ("<p>Ж</p>", None));
        assert_eq!(parse_cf_html(b"Version:0.9\r\n<p>x</p>").unwrap().fragment, "<p>x</p>");
        let out = parse_cf_html(b"Version:0.9\r\nStartFragment:999\r\nEndFragment:1000\r\n<p>y</p>").unwrap();
        assert_eq!(
            out.fragment, "<p>y</p>",
            "смещения вне буфера — документ после заголовка"
        );
        assert_eq!(parse_cf_html(b""), None);
        assert_eq!(parse_cf_html(b"Version:0.9\r\n"), None);
    }

    /// macOS (MAC-04): Word — фрагмент между отметками, Safari — `<meta charset>` без `<body>`, просто `<body>`;
    /// своё (`encode_mac_html`) — туда и обратно.
    #[test]
    fn parse_mac_documents() {
        let word = "<html xmlns:o=\"urn:x\"><head><style>td{}</style></head><body lang=RU>
<!--StartFragment--><table><tr><td>Ж</td></tr></table><!--EndFragment-->
</body></html>";
        assert_eq!(
            parse_mac_html(word).unwrap().fragment,
            "<table><tr><td>Ж</td></tr></table>"
        );
        let safari = "<meta charset='utf-8'><span style=\"color: red\">Город</span>";
        assert_eq!(
            parse_mac_html(safari).unwrap(),
            Html {
                fragment: "<span style=\"color: red\">Город</span>".into(),
                source_url: None
            }
        );
        assert_eq!(
            parse_mac_html("<HTML><BODY class=x><p>a</p></BODY></HTML>")
                .unwrap()
                .fragment,
            "<p>a</p>"
        );
        assert_eq!(
            parse_mac_html(&encode_mac_html("<b>Ж</b>")).unwrap().fragment,
            "<b>Ж</b>"
        );
        assert_eq!(parse_mac_html("  <meta charset='utf-8'>  "), None);
        assert_eq!(parse_mac_html(""), None);
    }

    /// Буфер Windows: записали HTML и текст — прочитали тот же фрагмент. Буфер общий для рабочего стола, поэтому
    /// тест не запускается по умолчанию: `cargo test -p pets-shell -- --ignored clipboard`.
    #[cfg(windows)]
    #[test]
    #[ignore = "пишет в буфер обмена рабочего стола"]
    fn clipboard_round_trip() {
        write_html("<b>Ж</b>", "Ж", None).unwrap();
        assert!(has_html());
        assert_eq!(
            read_html(DEFAULT_MAX).unwrap().map(|h| h.fragment).as_deref(),
            Some("<b>Ж</b>")
        );
        assert_eq!(read_html(8).unwrap(), None, "больше предела — не читаем");
    }
}
