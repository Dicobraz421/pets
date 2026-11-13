//! Журнал процесса: tracing в stderr, `log` через tracing-log; системный журнал (Windows «Приложение», macOS os_log,
//! Linux journald — PAR-03) — слоем
//! [`SystemLog`]: ERROR — «Ошибка», WARN — «Предупреждение», INFO — «Сведения», паники и фатальные ошибки — своими
//! кодами ([`EventId`]). Что пропустит фильтр (настройка «Уровень журнала» приложения), то и попадёт в системный
//! журнал, но не подробнее INFO: DEBUG и TRACE — только в файл. Какое событие какого уровня — `LOGGING.md` крейта.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::util::SubscriberInitExt as _;

use crate::sys;

/// Фильтр, если переменная не задана.
const DEFAULT_FILTER: &str = "info";
/// Цель события паники (хук [`init_logging`]): в системном журнале — [`EventId::Crash`].
pub const PANIC_TARGET: &str = "panic";
/// Цель фатальной ошибки запуска ([`fatal`]): [`EventId::StartupFailed`].
pub const FATAL_TARGET: &str = "fatal";

/// Код события системного журнала, общий для приложений Pets; важность (тип записи) — от кода.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EventId {
    /// Паника.
    Crash = 1000,
    /// Событие ERROR.
    Error = 1001,
    /// Запуск невозможен.
    StartupFailed = 1002,
    /// Событие WARN.
    Warning = 1003,
    /// Событие INFO.
    Info = 1004,
}

/// Тип записи системного журнала.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    Error,
    Warning,
    Info,
}

impl EventId {
    pub(crate) fn severity(self) -> Severity {
        match self {
            EventId::Crash | EventId::Error | EventId::StartupFailed => Severity::Error,
            EventId::Warning => Severity::Warning,
            EventId::Info => Severity::Info,
        }
    }

    /// Код события tracing по уровню и цели; DEBUG и TRACE — не для системного журнала.
    pub fn of(level: Level, target: &str) -> Option<EventId> {
        match level {
            Level::ERROR if target == PANIC_TARGET => Some(EventId::Crash),
            Level::ERROR if target == FATAL_TARGET => Some(EventId::StartupFailed),
            Level::ERROR => Some(EventId::Error),
            Level::WARN => Some(EventId::Warning),
            Level::INFO => Some(EventId::Info),
            _ => None,
        }
    }
}

/// Журнал: tracing в stderr (уровень из переменной `env_var`, напр. POSTMAN_LOG, по умолчанию info;
/// формат filter::Targets — "info,gpui=warn"), записи `log` (gpui) тоже идут сюда (tracing-log);
/// системный журнал — [`SystemLog`] с источником `app`, тот же фильтр.
/// Хук паники: сообщение+место → tracing::error!(target: "panic") и системный журнал, затем прежний хук.
///
/// Повторный вызов — без эффекта (подписчик уже стоит; пишется предупреждение).
pub fn init_logging(app: &str, env_var: &str) {
    let raw = std::env::var(env_var).ok();
    let (targets, bad) = match raw.as_deref().map(str::parse::<Targets>) {
        Some(Ok(t)) => (t, None),
        Some(Err(e)) => (default_targets(), Some(e)),
        None => (default_targets(), None),
    };
    let stderr = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal());
    let installed = tracing_subscriber::registry()
        .with(targets)
        .with(stderr)
        .with(SystemLog::new(app))
        .try_init();
    if let Err(e) = installed {
        tracing::warn!(target: "pets_shell", "журнал уже настроен: {e}");
        return;
    }
    if let Some(e) = bad {
        tracing::warn!(target: "pets_shell", "{env_var}: фильтр не разобран ({e}), взят «{DEFAULT_FILTER}»");
    }
    install_panic_hook();
}

/// Запуск невозможен: tracing::error! (→ системный журнал, [`EventId::StartupFailed`]), eprintln, exit(1).
pub fn fatal(what: &str, e: impl std::fmt::Display) -> ! {
    tracing::error!(target: "fatal", "{what}: {e}");
    eprintln!("{what}: {e}");
    std::process::exit(1)
}

/// Записать событие в системный журнал сразу, без tracing и без ограничения повторов (падение, записанное своим
/// обработчиком). Источник открывает [`SystemLog::new`] (или [`init_logging`]); до этого — ничего.
pub fn system_event(id: EventId, text: &str) {
    sys::report_event(id, text);
}

fn default_targets() -> Targets {
    Targets::new().with_default(Level::INFO)
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let msg = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("(не строка)");
        match info.location() {
            Some(l) => tracing::error!(target: "panic", "{msg} ({}:{}:{})", l.file(), l.line(), l.column()),
            None => tracing::error!(target: "panic", "{msg}"),
        }
        previous(info);
    }));
}

/// Слой tracing → системный журнал (Windows «Приложение», macOS os_log, Linux journald; источник — имя приложения):
/// ERROR, WARN и INFO, которые
/// пропустил фильтр подписчика (настройка «Уровень журнала»), — сообщение и поля одной строкой, код — [`EventId`].
/// Повторы ограничены ([`Throttle`]; у каждой важности свой счёт — сведения не вытесняют ошибок): ошибка, которая
/// сыплется на каждом кадре (gpui: «unable to get cursor position» при заблокированном экране), не забивает журнал.
/// Паники — всегда. Приложению со своим подписчиком (файл, Tracy): `registry().with(фильтр).with(SystemLog::new(имя))`.
pub struct SystemLog {
    throttle: Mutex<[Throttle; 3]>,
    skip: Vec<&'static str>,
}

impl SystemLog {
    /// Слой с источником `app` (источник открывается один раз за процесс).
    pub fn new(app: &str) -> SystemLog {
        sys::event_source(app);
        SystemLog {
            throttle: Mutex::default(),
            skip: Vec::new(),
        }
    }

    /// События этой цели не писать: приложение пишет их само ([`system_event`]).
    #[must_use]
    pub fn skip_target(mut self, target: &'static str) -> SystemLog {
        self.skip.push(target);
        self
    }

    /// Что записать: код и строку (с числом пропущенных повторов) или ничего.
    fn admit(&self, level: Level, target: &str, line: String, now: Instant) -> Option<(EventId, String)> {
        if self.skip.contains(&target) {
            return None;
        }
        let id = EventId::of(level, target)?;
        if id == EventId::Crash {
            return Some((id, line));
        }
        let slot = match id.severity() {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
        };
        let mut t = self.throttle.lock().unwrap_or_else(PoisonError::into_inner);
        t[slot].admit(line, now).map(|l| (id, l))
    }
}

impl<S: Subscriber> Layer<S> for SystemLog {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        if *meta.level() > Level::INFO {
            return;
        }
        if let Some((id, line)) = self.admit(*meta.level(), meta.target(), event_line(event), Instant::now()) {
            sys::report_event(id, &line);
        }
    }
}

/// Повтор той же строки — не чаще раза в [`Throttle::REPEAT`] (при следующей записи — «повторов: N»); всего —
/// не больше [`Throttle::PER_MINUTE`] записей в минуту.
#[derive(Debug, Default)]
struct Throttle {
    /// Строка → когда записана и сколько повторов с тех пор пропущено.
    seen: HashMap<String, (Instant, u32)>,
    minute: Option<Instant>,
    in_minute: u32,
}

impl Throttle {
    const REPEAT: Duration = Duration::from_secs(600);
    const PER_MINUTE: u32 = 20;
    /// Сколько разных строк помнить (дальше — забыть самые старые).
    const REMEMBER: usize = 256;

    /// Что записать в журнал: строку (с числом пропущенных повторов) или ничего.
    fn admit(&mut self, line: String, now: Instant) -> Option<String> {
        if let Some((at, skipped)) = self.seen.get_mut(&line)
            && now.duration_since(*at) < Self::REPEAT
        {
            *skipped = skipped.saturating_add(1);
            return None;
        }
        if self
            .minute
            .is_none_or(|m| now.duration_since(m) >= Duration::from_secs(60))
        {
            self.minute = Some(now);
            self.in_minute = 0;
        }
        if self.in_minute >= Self::PER_MINUTE {
            return None;
        }
        self.in_minute += 1;
        let skipped = self.seen.insert(line.clone(), (now, 0)).map_or(0, |(_, n)| n);
        if self.seen.len() > Self::REMEMBER
            && let Some(oldest) = self.seen.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone())
        {
            self.seen.remove(&oldest);
        }
        Some(if skipped > 0 {
            format!("{line} (repeated {skipped} times)")
        } else {
            line
        })
    }
}

/// Строка события: `цель: сообщение имя=значение …`.
fn event_line(event: &Event<'_>) -> String {
    let mut f = Fields::default();
    event.record(&mut f);
    format!("{}: {}{}", event.metadata().target(), f.message, f.rest)
}

/// Сборщик полей: `message` отдельно, прочие — ` имя=значение`.
#[derive(Default)]
struct Fields {
    message: String,
    rest: String,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.rest, " {}={value}", field.name());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.rest, " {}={value:?}", field.name());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Слой-перехватчик: строка, которую получил бы журнал Windows.
    struct Capture(Arc<Mutex<Vec<String>>>);

    impl<S: Subscriber> Layer<S> for Capture {
        fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
            self.0.lock().unwrap().push(event_line(event));
        }
    }

    #[test]
    fn repeats_are_throttled() {
        let mut t = Throttle::default();
        let t0 = Instant::now();
        let line = || "pets-gpui-windows::window: unable to get cursor position".to_string();
        assert!(t.admit(line(), t0).is_some());
        for s in 1..50 {
            assert!(t.admit(line(), t0 + Duration::from_secs(s)).is_none());
        }
        let again = t.admit(line(), t0 + Throttle::REPEAT).unwrap();
        assert!(again.ends_with("(repeated 49 times)"), "{again}");
        // Разные строки — не больше PER_MINUTE в минуту.
        let t1 = t0 + Throttle::REPEAT * 2;
        let admitted = (0..100).filter(|i| t.admit(format!("e{i}"), t1).is_some()).count();
        assert_eq!(admitted, Throttle::PER_MINUTE as usize);
        assert!(t.admit("next".into(), t1 + Duration::from_secs(61)).is_some());
        assert!(t.seen.len() <= Throttle::REMEMBER + 1);
    }

    /// Код и важность — по уровню и цели; DEBUG — мимо; у важностей свой счёт повторов; паника — всегда.
    #[test]
    fn levels_map_to_event_ids() {
        assert_eq!(EventId::of(Level::ERROR, "imap"), Some(EventId::Error));
        assert_eq!(EventId::of(Level::ERROR, PANIC_TARGET), Some(EventId::Crash));
        assert_eq!(EventId::of(Level::ERROR, FATAL_TARGET), Some(EventId::StartupFailed));
        assert_eq!(EventId::of(Level::WARN, "imap"), Some(EventId::Warning));
        assert_eq!(EventId::of(Level::INFO, "app"), Some(EventId::Info));
        assert_eq!(
            (EventId::of(Level::DEBUG, "app"), EventId::of(Level::TRACE, "app")),
            (None, None)
        );
        assert_eq!(EventId::Warning.severity(), Severity::Warning);
        assert_eq!(EventId::StartupFailed as u32, 1002);

        let log = SystemLog {
            throttle: Mutex::default(),
            skip: vec!["urfin::crash"],
        };
        let now = Instant::now();
        for i in 0..Throttle::PER_MINUTE {
            assert!(log.admit(Level::INFO, "app", format!("i{i}"), now).is_some());
        }
        assert!(
            log.admit(Level::INFO, "app", "лишнее".into(), now).is_none(),
            "у сведений свой предел"
        );
        assert_eq!(
            log.admit(Level::ERROR, "imap", "ошибка".into(), now),
            Some((EventId::Error, "ошибка".into()))
        );
        assert_eq!(
            log.admit(Level::WARN, "imap", "отказ".into(), now).map(|x| x.0),
            Some(EventId::Warning)
        );
        assert!(log.admit(Level::ERROR, "urfin::crash", "сам".into(), now).is_none());
        assert!(log.admit(Level::DEBUG, "app", "детали".into(), now).is_none());
        for _ in 0..3 {
            assert!(log.admit(Level::ERROR, PANIC_TARGET, "паника".into(), now).is_some());
        }
    }

    #[test]
    fn fields_are_formatted_in_one_line() {
        let out = Arc::new(Mutex::new(Vec::new()));
        let sub = tracing_subscriber::registry().with(Capture(out.clone()));
        tracing::subscriber::with_default(sub, || {
            tracing::error!(target: "imap", account = "work", code = 7, "нет связи: {}", "таймаут");
        });
        assert_eq!(*out.lock().unwrap(), ["imap: нет связи: таймаут account=work code=7"]);
    }
}
