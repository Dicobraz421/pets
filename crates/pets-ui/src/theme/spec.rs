//! Тема в TOML: разбор с проверкой и обратная запись.
//!
//! ```toml
//! name = "Моя тема"          # название в списке; нет — id (имя файла)
//! appearance = "dark"         # light | dark: тема только такая; нет — светлая/тёмная как в ОС
//! accent = "system"           # system | "#RRGGBB": от него считаются роли accent.*
//! density = 24                # 20 | 24 | 28 | 32
//! [tokens.light]              # роли светлого варианта, как `[ui.tokens.light]` Урфина
//! "accent.fill" = "#0067C0"
//! [tokens.dark]
//! "text.primary" = "#D2D2D2"
//! "app.label.blue" = "#7DB8E8"  # роль приложения (`app.*`), если приложение её объявило
//! ```
//! Ключи ролей — с точками в кавычках; вложенные таблицы тоже читаются (`[tokens.dark.text] primary = …`).
//! Неизвестные роли и ключи — ошибка со списком; роли, которых нет, берутся из базовой темы.

use std::collections::BTreeMap;
use std::fmt;

use super::color::Color;
use super::roles::{Density, Role};
use super::{AppRole, Appearance};

/// Откуда акцент.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Accent {
    /// Акцент ОС.
    System,
    /// Свой базовый цвет; оттенки — в OKLCH.
    Color(Color),
}

/// Тема или слой поверх неё (переопределения пользователя). `None` — не задано, берётся из слоя ниже.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ThemeSpec {
    /// Идентификатор: имя файла без `.toml` (ASCII lower-case) или ключ `id`.
    pub id: String,
    pub name: Option<String>,
    /// Только светлая или только тёмная. Высокую контрастность в файле не задают — она от ОС.
    pub appearance: Option<Appearance>,
    pub accent: Option<Accent>,
    pub density: Option<Density>,
    /// Роль (`text.primary`, `app.…`) → цвет светлого варианта.
    pub light: BTreeMap<String, Color>,
    pub dark: BTreeMap<String, Color>,
}

/// Ошибка в файле темы.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Issue {
    /// Ключ вне схемы (`tokens.sepia`, `colour`).
    UnknownKey(String),
    /// Ролей нет ни в pets-ui, ни среди ролей приложения.
    UnknownRoles {
        table: &'static str,
        roles: Vec<String>,
    },
    BadColor {
        table: &'static str,
        role: String,
        value: String,
    },
    BadValue {
        key: &'static str,
        value: String,
    },
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Issue::UnknownKey(k) => write!(f, "unknown key `{k}`"),
            Issue::UnknownRoles { table, roles } => {
                write!(f, "unknown roles in [tokens.{table}]: {}", roles.join(", "))
            }
            Issue::BadColor { table, role, value } => {
                write!(
                    f,
                    "[tokens.{table}] {role}: bad color {value} (want \"#RRGGBB\" or \"#AARRGGBB\")"
                )
            }
            Issue::BadValue { key, value } => write!(f, "{key}: bad value {value}"),
        }
    }
}

/// Ошибки тем (тексты — для журнала, по-английски; пользователю — свой текст через Fluent).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    #[error("theme `{id}`: TOML syntax: {msg}")]
    Syntax { id: String, msg: String },
    #[error("theme `{id}`: {}", issues.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Invalid { id: String, issues: Vec<Issue> },
    #[error("theme file {path}: {msg}")]
    Io { path: String, msg: String },
    #[error("theme `{0}`: id is taken by a built-in theme")]
    Reserved(String),
    #[error("unknown theme `{0}`")]
    Unknown(String),
    #[error("theme change is locked by policy")]
    Locked,
}

impl ThemeSpec {
    /// Разбор файла темы. `id` — имя по умолчанию (имя файла), `app` — роли приложения (`app.*`).
    pub fn parse(id: &str, text: &str, app: &[AppRole]) -> Result<ThemeSpec, ThemeError> {
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| ThemeError::Syntax {
            id: id.to_owned(),
            msg: e.message().to_owned(),
        })?;
        let mut issues = Vec::new();
        let mut spec = ThemeSpec {
            id: id.to_owned(),
            ..ThemeSpec::default()
        };
        for (key, value) in &table {
            match key.as_str() {
                "id" => match value.as_str() {
                    Some(s) if valid_id(s) => s.clone_into(&mut spec.id),
                    _ => issues.push(bad("id", value)),
                },
                "name" => match value.as_str() {
                    Some(s) => spec.name = Some(s.to_owned()),
                    None => issues.push(bad("name", value)),
                },
                "appearance" => match value.as_str() {
                    Some("light") => spec.appearance = Some(Appearance::Light),
                    Some("dark") => spec.appearance = Some(Appearance::Dark),
                    _ => issues.push(bad("appearance", value)),
                },
                "accent" => match value.as_str() {
                    Some(s) if s.eq_ignore_ascii_case("system") => spec.accent = Some(Accent::System),
                    Some(s) => match Color::parse(s) {
                        Some(c) => spec.accent = Some(Accent::Color(c.opaque())),
                        None => issues.push(bad("accent", value)),
                    },
                    None => issues.push(bad("accent", value)),
                },
                "density" => match value.as_integer().and_then(Density::from_row) {
                    Some(d) => spec.density = Some(d),
                    None => issues.push(bad("density", value)),
                },
                "tokens" => match value.as_table() {
                    Some(tokens) => {
                        for (variant, roles) in tokens {
                            let (name, target) = match variant.as_str() {
                                "light" => ("light", &mut spec.light),
                                "dark" => ("dark", &mut spec.dark),
                                other => {
                                    issues.push(Issue::UnknownKey(format!("tokens.{other}")));
                                    continue;
                                }
                            };
                            match roles.as_table() {
                                Some(roles) => read_roles(name, roles, app, target, &mut issues),
                                None => issues.push(Issue::UnknownKey(format!("tokens.{name}"))),
                            }
                        }
                    }
                    None => issues.push(Issue::UnknownKey("tokens".into())),
                },
                other => issues.push(Issue::UnknownKey(other.to_owned())),
            }
        }
        if issues.is_empty() {
            Ok(spec)
        } else {
            Err(ThemeError::Invalid { id: spec.id, issues })
        }
    }

    /// Обратно в TOML (ключи по алфавиту); `parse(to_toml())` даёт ту же тему.
    pub fn to_toml(&self) -> String {
        use fmt::Write as _;
        let q = |s: &str| toml::Value::String(s.to_owned()).to_string();
        let mut out = String::new();
        let _ = writeln!(out, "id = {}", q(&self.id));
        if let Some(name) = &self.name {
            let _ = writeln!(out, "name = {}", q(name));
        }
        match self.appearance {
            Some(Appearance::Light) => out.push_str("appearance = \"light\"\n"),
            Some(Appearance::Dark) => out.push_str("appearance = \"dark\"\n"),
            Some(Appearance::HighContrast) | None => {}
        }
        match self.accent {
            Some(Accent::System) => out.push_str("accent = \"system\"\n"),
            Some(Accent::Color(c)) => {
                let _ = writeln!(out, "accent = \"{}\"", c.hex());
            }
            None => {}
        }
        if let Some(d) = self.density {
            let _ = writeln!(out, "density = {}", d.row());
        }
        for (name, roles) in [("light", &self.light), ("dark", &self.dark)] {
            if roles.is_empty() {
                continue;
            }
            let _ = writeln!(out, "\n[tokens.{name}]");
            for (role, c) in roles {
                let _ = writeln!(out, "{} = \"{}\"", q(role), c.hex());
            }
        }
        out
    }
}

/// `[a-z0-9-]+`: id — это и имя файла.
pub(crate) fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn bad(key: &'static str, value: &toml::Value) -> Issue {
    Issue::BadValue {
        key,
        value: value.to_string(),
    }
}

fn read_roles(
    table: &'static str,
    roles: &toml::Table,
    app: &[AppRole],
    out: &mut BTreeMap<String, Color>,
    issues: &mut Vec<Issue>,
) {
    let mut flat = Vec::new();
    flatten("", roles, &mut flat);
    let mut unknown = Vec::new();
    for (role, value) in flat {
        let known = Role::from_name(&role).is_some() || app.iter().any(|a| a.name == role);
        if !known {
            unknown.push(role);
            continue;
        }
        match value.as_str().and_then(Color::parse) {
            Some(c) => {
                out.insert(role, c);
            }
            None => issues.push(Issue::BadColor {
                table,
                role,
                value: value.to_string(),
            }),
        }
    }
    if !unknown.is_empty() {
        issues.push(Issue::UnknownRoles { table, roles: unknown });
    }
}

/// Вложенные таблицы → имена с точками (`[tokens.dark.text] primary` → `text.primary`).
fn flatten<'a>(prefix: &str, t: &'a toml::Table, out: &mut Vec<(String, &'a toml::Value)>) {
    for (k, v) in t {
        let name = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v.as_table() {
            Some(sub) => flatten(&name, sub, out),
            None => out.push((name, v)),
        }
    }
}
