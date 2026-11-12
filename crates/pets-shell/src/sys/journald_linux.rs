//! Системный журнал Linux — systemd-journald: родной протокол дейтаграммами в `/run/systemd/journal/socket`, без
//! `libsystemd` (PAR-03). Поля: `MESSAGE`, `PRIORITY` (паника — 2 «crit», ошибка — 3, предупреждение — 4, сведения —
//! 6), `SYSLOG_IDENTIFIER` — имя приложения, `PETS_EVENT_ID` — код [`EventId`]. Просмотр: `journalctl -t <app>`.
//! Нет journald (контейнер, другая система инициализации) — ничего, как без источника на Windows.

use std::os::unix::net::UnixDatagram;
use std::sync::OnceLock;

use crate::logging::EventId;

const SOCKET: &str = "/run/systemd/journal/socket";
/// Предел текста записи в байтах (как у журнала Windows — ~31 тыс. знаков); длиннее — обрезается по границе знака.
const MAX_TEXT: usize = 31_000;

struct Journal {
    socket: UnixDatagram,
    app: String,
}

static JOURNAL: OnceLock<Option<Journal>> = OnceLock::new();

/// Открыть сокет journald для `app` (один раз за процесс).
pub(crate) fn event_source(app: &str) {
    JOURNAL.get_or_init(|| {
        let socket = UnixDatagram::unbound().ok()?;
        socket.connect(SOCKET).ok()?;
        Some(Journal {
            socket,
            app: app.to_owned(),
        })
    });
}

/// Записать событие в journald; без сокета — ничего. Сбой отправки не пишется в журнал (запись журнала — сама
/// запись журнала).
pub(crate) fn report_event(id: EventId, text: &str) {
    let Some(Some(journal)) = JOURNAL.get() else {
        return;
    };
    let _ = journal.socket.send(&datagram(&journal.app, id, text));
}

fn priority(id: EventId) -> &'static str {
    match id {
        EventId::Crash => "2",
        EventId::Error | EventId::StartupFailed => "3",
        EventId::Warning => "4",
        EventId::Info => "6",
    }
}

/// Дейтаграмма родного протокола: `ИМЯ\n` + длина (u64 LE) + значение + `\n` — значение может содержать перевод строки.
fn datagram(app: &str, id: EventId, text: &str) -> Vec<u8> {
    let mut end = text.len().min(MAX_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let code = (id as u32).to_string();
    let fields: [(&str, &str); 4] = [
        ("MESSAGE", &text[..end]),
        ("PRIORITY", priority(id)),
        ("SYSLOG_IDENTIFIER", app),
        ("PETS_EVENT_ID", &code),
    ];
    let mut out = Vec::with_capacity(end + 128);
    for (name, value) in fields {
        out.extend_from_slice(name.as_bytes());
        out.push(b'\n');
        out.extend_from_slice(&(value.len() as u64).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
        out.push(b'\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Поле — имя, длина LE, значение с переводом строки как есть; длинный текст обрезан по границе знака.
    #[test]
    fn journald_datagram_fields() {
        let d = datagram("Postman", EventId::Warning, "a\nb");
        let mut want = b"MESSAGE\n".to_vec();
        want.extend_from_slice(&3u64.to_le_bytes());
        want.extend_from_slice(b"a\nb\nPRIORITY\n");
        want.extend_from_slice(&1u64.to_le_bytes());
        want.extend_from_slice(b"4\nSYSLOG_IDENTIFIER\n");
        want.extend_from_slice(&7u64.to_le_bytes());
        want.extend_from_slice(b"Postman\nPETS_EVENT_ID\n");
        want.extend_from_slice(&4u64.to_le_bytes());
        want.extend_from_slice(b"1003\n");
        assert_eq!(d, want);

        let long = "я".repeat(MAX_TEXT);
        let d = datagram("x", EventId::Crash, &long);
        let len = u64::from_le_bytes(d[8..16].try_into().unwrap()) as usize;
        assert!(len <= MAX_TEXT && len.is_multiple_of(2), "{len}");
        assert_eq!(priority(EventId::StartupFailed), "3");
    }

    /// Без journald (контейнер CI) — тихо; с ним — запись уходит без ошибки.
    #[test]
    fn journald_report_without_panic() {
        event_source("PetsShellTest");
        report_event(EventId::Info, "pets-shell test event");
    }
}
