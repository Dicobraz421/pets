//! Тексты интерфейса на Fluent. Наборы `.ftl` вшиты в приложение и библиотеки (по набору на язык:
//! RU, EN), [`I18n`] собирает их в цепочку «выбранный язык → [`FALLBACK`]». Активный язык —
//! глобальный ([`init`], [`set_language`], [`current`], [`tr!`]), меняется на лету; [`epoch`] — для
//! сброса кэшей текста. Для тестов приложения: [`scoped`] (язык одного потока), [`check`] (наборы
//! языков согласованы) и [`raw_strings`] (в коде нет сырых строк интерфейса).
//!
//! Крейт без UI. Изоляция подстановок (U+2068/U+2069) выключена: шрифты gpui рисуют их квадратиками.

mod check;
mod raw;

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use fluent_bundle::FluentResource;
use fluent_bundle::concurrent::FluentBundle;

pub use check::check;
pub use fluent_bundle::{FluentArgs, FluentValue};
pub use raw::{RawString, raw_strings};

type Bundle = FluentBundle<FluentResource>;

/// Набор текстов: язык (BCP-47, "ru"/"en") и исходник .ftl.
pub type Resource = (&'static str, &'static str);

/// Язык по умолчанию и последнее звено цепочки.
pub const FALLBACK: &str = "en";

/// Неприятность в наборах или при выборе языка. Не фатальна: битое пропускается, остальное работает.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    #[error("{lang}: строка {line}: {message}")]
    Parse { lang: String, line: usize, message: String },
    #[error("«{0}» — не тег языка")]
    BadLanguage(String),
    #[error("{lang}: ключ «{key}» повторяется")]
    Duplicate { lang: String, key: String },
    #[error("ключ «{key}» есть в {present}, нет в {missing}")]
    Missing {
        key: String,
        present: String,
        missing: String,
    },
    #[error("ключ «{key}»: переменные в {lang} — {vars}, в {other} — {other_vars}")]
    Variables {
        key: String,
        lang: String,
        vars: String,
        other: String,
        other_vars: String,
    },
    #[error("язык «{requested}» не найден, выбран {used}")]
    UnknownLanguage { requested: String, used: String },
}

/// Тексты одного языка: цепочка бандлов «выбранный → [`FALLBACK`]».
pub struct I18n {
    lang: String,
    chain: Vec<Bundle>,
}

// Глобальный снимок отдаётся между потоками.
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<I18n>();
};

impl std::fmt::Debug for I18n {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("I18n")
            .field("lang", &self.lang)
            .field("bundles", &self.chain.len())
            .finish()
    }
}

impl I18n {
    /// Собрать для языка `lang` ("auto" — язык ОС) из наборов приложения и библиотек.
    /// Ошибки разбора и дубли ключей — в Vec<Problem>, текст при этом работает.
    pub fn new(resources: &[&[Resource]], lang: &str) -> (Self, Vec<Problem>) {
        let mut problems = Vec::new();
        let langs = languages(resources);
        let (chosen, unknown) = resolve(&langs, lang);
        problems.extend(unknown);
        let mut tags = vec![chosen.as_str()];
        if !chosen.eq_ignore_ascii_case(FALLBACK) {
            tags.push(FALLBACK);
        }
        let chain = tags
            .into_iter()
            .filter(|tag| langs.iter().any(|l| l.eq_ignore_ascii_case(tag)))
            .filter_map(|tag| bundle(tag, resources, &mut problems))
            .collect();
        (Self { lang: chosen, chain }, problems)
    }

    /// Пустой: `get` отдаёт ключ. Глобальное состояние до [`init`].
    fn empty() -> Self {
        Self {
            lang: FALLBACK.to_owned(),
            chain: Vec::new(),
        }
    }

    /// Выбранный язык — тег из наборов.
    pub fn lang(&self) -> &str {
        &self.lang
    }

    /// Текст по ключу; нет ключа — из [`FALLBACK`], затем сам ключ.
    pub fn get(&self, key: &str) -> String {
        self.format(key, None)
    }

    /// Текст с аргументами: числа выбирают плюраль по языку сообщения.
    pub fn fmt(&self, key: &str, args: &FluentArgs) -> String {
        self.format(key, Some(args))
    }

    fn format(&self, key: &str, args: Option<&FluentArgs>) -> String {
        for bundle in &self.chain {
            let Some(pattern) = bundle.get_message(key).and_then(|m| m.value()) else {
                continue;
            };
            let mut errors = Vec::new();
            let text = bundle.format_pattern(pattern, args, &mut errors).into_owned();
            for e in &errors {
                tracing::warn!(key, lang = %self.lang, %e, "i18n");
            }
            return text;
        }
        key.to_owned()
    }
}

/// Языки наборов в порядке появления, без повторов.
fn languages(resources: &[&[Resource]]) -> Vec<&'static str> {
    let mut langs: Vec<&'static str> = Vec::new();
    for &(lang, _) in resources.iter().flat_map(|set| set.iter()) {
        if !langs.iter().any(|l| l.eq_ignore_ascii_case(lang)) {
            langs.push(lang);
        }
    }
    langs
}

/// Язык для запроса: точный тег, затем первичный подтег ("ru-RU" → "ru"); иначе [`FALLBACK`], иначе
/// первый из наборов. Неизвестный явный язык — [`Problem::UnknownLanguage`]; язык ОС — без неё.
fn resolve(langs: &[&str], requested: &str) -> (String, Option<Problem>) {
    let requested = requested.trim();
    let auto = requested.is_empty() || requested.eq_ignore_ascii_case("auto");
    let wanted = if auto { os_language() } else { requested.to_owned() };
    let primary = primary(&wanted);
    let found = langs
        .iter()
        .find(|l| l.eq_ignore_ascii_case(&wanted))
        .or_else(|| langs.iter().find(|l| l.eq_ignore_ascii_case(primary)));
    if let Some(found) = found {
        return ((*found).to_owned(), None);
    }
    let used = langs
        .iter()
        .find(|l| l.eq_ignore_ascii_case(FALLBACK))
        .or(langs.first())
        .map_or(FALLBACK, |l| *l)
        .to_owned();
    let problem = (!auto).then(|| Problem::UnknownLanguage {
        requested: requested.to_owned(),
        used: used.clone(),
    });
    (used, problem)
}

/// Первичный подтег: "ru-RU" → "ru"; на Unix бывает `ru_RU.UTF-8`.
fn primary(tag: &str) -> &str {
    tag.split(['-', '_', '.', '@']).next().unwrap_or_default()
}

/// Разобрать исходник; ошибки разбора — в `problems`, разобранное остаётся.
fn parse(lang: &str, source: &str, problems: &mut Vec<Problem>) -> FluentResource {
    match FluentResource::try_new(source.to_owned()) {
        Ok(resource) => resource,
        Err((resource, errors)) => {
            for e in errors {
                let at = e.pos.start.min(source.len());
                let line = source.as_bytes()[..at].iter().filter(|&&b| b == b'\n').count() + 1;
                problems.push(Problem::Parse {
                    lang: lang.to_owned(),
                    line,
                    message: e.to_string(),
                });
            }
            resource
        }
    }
}

/// Бандл языка из всех его наборов; при дубле действует первое определение.
fn bundle(lang: &str, resources: &[&[Resource]], problems: &mut Vec<Problem>) -> Option<Bundle> {
    let Ok(id) = lang.parse::<unic_langid::LanguageIdentifier>() else {
        problems.push(Problem::BadLanguage(lang.to_owned()));
        return None;
    };
    let mut bundle = Bundle::new_concurrent(vec![id]);
    bundle.set_use_isolating(false);
    for &(l, source) in resources.iter().flat_map(|set| set.iter()) {
        if !l.eq_ignore_ascii_case(lang) {
            continue;
        }
        let resource = parse(lang, source, problems);
        if let Err(errors) = bundle.add_resource(resource) {
            for e in errors {
                match e {
                    fluent_bundle::FluentError::Overriding { id, .. } => problems.push(Problem::Duplicate {
                        lang: lang.to_owned(),
                        key: id,
                    }),
                    e => tracing::warn!(lang, %e, "i18n"),
                }
            }
        }
    }
    Some(bundle)
}

/// Язык ОС — первичный подтег ("ru-RU" → "ru"), иначе [`FALLBACK`].
pub fn os_language() -> String {
    sys_locale::get_locale()
        .map(|tag| primary(&tag).to_ascii_lowercase())
        .filter(|tag| !tag.is_empty())
        .unwrap_or_else(|| FALLBACK.to_owned())
}

struct Global {
    /// Наборы из [`init`]: [`set_language`] пересобирает из них.
    resources: Vec<Resource>,
    active: Arc<I18n>,
}

/// До [`init`] — пустой: `get` отдаёт ключ.
static GLOBAL: LazyLock<RwLock<Global>> = LazyLock::new(|| {
    RwLock::new(Global {
        resources: Vec::new(),
        active: Arc::new(I18n::empty()),
    })
});

static EPOCH: AtomicU64 = AtomicU64::new(0);

/// Включить глобальный активный язык `lang` ("auto" — язык ОС) из наборов приложения и библиотек.
pub fn init(resources: &[&[Resource]], lang: &str) -> Vec<Problem> {
    let flat: Vec<Resource> = resources.iter().flat_map(|set| set.iter().copied()).collect();
    activate(flat, lang)
}

/// Сменить язык на лету, наборы те же. Читатели видят старый или новый язык целиком.
pub fn set_language(lang: &str) -> Vec<Problem> {
    let resources = GLOBAL.read().unwrap_or_else(PoisonError::into_inner).resources.clone();
    activate(resources, lang)
}

fn activate(resources: Vec<Resource>, lang: &str) -> Vec<Problem> {
    let (active, problems) = I18n::new(&[&resources], lang);
    for p in &problems {
        tracing::warn!(%p, "i18n");
    }
    {
        let mut global = GLOBAL.write().unwrap_or_else(PoisonError::into_inner);
        global.resources = resources;
        global.active = Arc::new(active);
        EPOCH.fetch_add(1, Ordering::Release);
    }
    problems
}

thread_local! {
    /// Языки потока ([`scoped`]): действует последний. Пусто — глобальный язык.
    static THREAD: RefCell<Vec<Arc<I18n>>> = const { RefCell::new(Vec::new()) };
}

/// Язык `lang` только для текущего потока, пока живёт значок: тесты приложения в одном процессе
/// включают разные языки и не мешают друг другу. Вложения допустимы — действует последний.
/// [`epoch`] не меняется, наборы глобального языка — тоже.
///
/// Значок привязывается именем: `let _ru = scoped(&TEXTS, "ru");`. `let _ = scoped(..)` гасит язык сразу.
pub fn scoped(resources: &[&[Resource]], lang: &str) -> Scoped {
    let flat: Vec<Resource> = resources.iter().flat_map(|set| set.iter().copied()).collect();
    let (active, problems) = I18n::new(&[&flat], lang);
    for p in &problems {
        tracing::warn!(%p, "i18n");
    }
    THREAD.with(|t| t.borrow_mut().push(Arc::new(active)));
    Scoped {
        thread: PhantomData::<*mut ()>,
    }
}

/// Значок языка потока — живёт до конца области, гасить нельзя (`let _ru = scoped(..)`).
#[must_use = "язык действует, пока значок жив: let _ru = scoped(..)"]
pub struct Scoped {
    /// Нельзя унести в другой поток: там он снял бы чужой язык.
    thread: PhantomData<*mut ()>,
}

impl Drop for Scoped {
    fn drop(&mut self) {
        THREAD.with(|t| {
            let _ = t.borrow_mut().pop();
        });
    }
}

/// Активный язык (снимок): языка потока нет — глобальный.
pub fn current() -> Arc<I18n> {
    if let Some(active) = THREAD.with(|t| t.borrow().last().cloned()) {
        return active;
    }
    Arc::clone(&GLOBAL.read().unwrap_or_else(PoisonError::into_inner).active)
}

/// Номер смены языка: растёт при каждой; кэши текста сравнивают его со своим.
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Acquire)
}

/// Текст активного языка: `tr!("key")`, `tr!("key", n = 3, name = "Аня")`.
#[macro_export]
macro_rules! tr {
    ($key:expr $(,)?) => {
        $crate::current().get($key)
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {{
        let mut args = $crate::FluentArgs::new();
        $(args.set(stringify!($name), $value);)+
        $crate::current().fmt($key, &args)
    }};
}

#[cfg(test)]
mod tests;
