//! Журнал процесса: tracing в stderr, `log` через tracing-log, ERROR и паники — ещё и в журнал Windows.

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

/// Журнал: tracing в stderr (уровень из переменной `env_var`, напр. POSTMAN_LOG, по умолчанию info;
/// формат filter::Targets — "info,gpui=warn"), записи `log` (gpui) тоже идут сюда (tracing-log);
/// события ERROR и паники дублируются в журнал Windows «Приложение» с источником `app`.
/// Хук паники: сообщение+место → tracing::error!(target: "panic") и журнал Windows, затем прежний хук.
///
/// Повторный вызов — без эффекта (подписчик уже стоит; пишется предупреждение).
pub fn init_logging(app: &str, env_var: &str) {
    let raw = std::env::var(env_var).ok();
    let (targets, bad) = match raw.as_deref().map(str::parse::<Targets>) {
        Some(Ok(t)) => (t, None),
        Some(Err(e)) => (default_targets(), Some(e)),
        None => (default_targets(), None),
    };
    sys::event_source(app);
    let stderr = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal());
    let installed = tracing_subscriber::registry()
        .with(targets)
        .with(stderr)
        .with(EventLogLayer::default())
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

/// Запуск невозможен: tracing::error! (→ журнал Windows), eprintln, exit(1).
pub fn fatal(what: &str, e: impl std::fmt::Display) -> ! {
    tracing::error!(target: "fatal", "{what}: {e}");
    eprintln!("{what}: {e}");
    std::process::exit(1)
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

/// Слой tracing: события ERROR → журнал Windows (сообщение и поля одной строкой), с ограничением повторов
/// ([`Throttle`]): ошибка, которая сыплется на каждом кадре (gpui: «unable to get cursor position» при
/// заблокированном экране), не забивает журнал.
#[derive(Default)]
struct EventLogLayer {
    throttle: Mutex<Throttle>,
}

impl<S: Subscriber> Layer<S> for EventLogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() != Level::ERROR {
            return;
        }
        let line = event_line(event);
        // Паники — всегда: их мало, и каждая важна.
        let line = if event.metadata().target() == "panic" {
            Some(line)
        } else {
            self.throttle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .admit(line, Instant::now())
        };
        if let Some(line) = line {
            sys::report_error(&line);
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
