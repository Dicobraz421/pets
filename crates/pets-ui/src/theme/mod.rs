//! Темы оформления: цвета ролей, метрики плотности, режим «как в ОС», темы в TOML, политика.
//!
//! Роли и метрики — те же имена и значения, что у токенов Урфина (`crates/ui/src/tokens`), формат
//! переопределений — его же `[tokens.light]` / `[tokens.dark]`. Слои темы (снизу вверх):
//! 1. Fluent светлая или тёмная (WinUI 3) — все роли;
//! 2. акцент (ОС или свой цвет): роли `accent.*` с гарантией контраста;
//! 3. таблица темы ([`ThemeSpec`]) для её варианта;
//! 4. переопределения пользователя (тот же формат).
//!
//! Режим высокой контрастности ОС сильнее всего: только системные цвета, слои 2–4 не действуют.
//! В интерфейсе — [`Themes`] (gpui `Global`): `Theme::global(cx).c(Role::TextPrimary)`.

mod accent;
mod builtin;
mod color;
mod policy;
mod roles;
mod spec;
mod state;
mod sys;

use std::sync::LazyLock;

use gpui_kit::{App, Hsla, Pixels, px};

pub use accent::{AccentPalette, DEFAULT_ACCENT};
pub use builtin::HcColors;
pub use color::{Color, contrast, over};
pub use policy::{POLICY_KEY, Policy};
pub use roles::{Density, FontSizes, Metric, Role};
pub use spec::{Accent, Issue, ThemeError, ThemeSpec};
pub use state::{ThemeConfig, ThemeState, Themes, load_dir, title};

use color::{contrast_c, lerp, over_c};

/// Минимальный контраст текста (WCAG AA).
pub const MIN_CONTRAST: f32 = 4.5;

/// «Как в ОС»: Fluent светлая/тёмная/высокая контрастность и акцент — из Windows, на лету.
pub const SYSTEM: &str = "system";
pub const FLUENT_LIGHT: &str = "fluent-light";
pub const FLUENT_DARK: &str = "fluent-dark";
/// Высокая контрастность: системные цвета, если режим ОС включён, иначе классическая чёрная.
pub const HIGH_CONTRAST: &str = "high-contrast";
/// Встроенные темы по порядку списка.
pub const BUILTIN: &[&str] = &[SYSTEM, FLUENT_LIGHT, FLUENT_DARK, HIGH_CONTRAST];

/// Вариант темы.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum Appearance {
    #[default]
    Light,
    Dark,
    HighContrast,
}

/// Роль приложения: имя `app.…`, значения для светлой и тёмной, при высокой контрастности —
/// значение роли pets-ui `hc` (только системные цвета).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AppRole {
    pub name: &'static str,
    pub light: Color,
    pub dark: Color,
    pub hc: Role,
}

/// Что ОС говорит о виде окон.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct OsLook {
    /// `AppsUseLightTheme = 0`.
    pub dark: bool,
    /// `SPI_GETHIGHCONTRAST` с `HCF_HIGHCONTRASTON`.
    pub high_contrast: bool,
    pub accent: AccentPalette,
    /// Системные цвета, когда включена высокая контрастность.
    pub hc: Option<HcColors>,
}

impl OsLook {
    /// Прочитать из ОС (реестр, `SystemParametersInfo`, `GetSysColor`): микросекунды, без файлов.
    pub fn read() -> OsLook {
        sys::os_look()
    }
}

/// Итоговая тема: цвет каждой роли, метрики, роли приложения.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    id: String,
    name: Option<String>,
    appearance: Appearance,
    density: Density,
    colors: [Hsla; Role::COUNT],
    metrics: [f32; Metric::COUNT],
    app: Vec<(&'static str, Hsla)>,
}

static FALLBACK: LazyLock<Theme> = LazyLock::new(|| {
    resolve(
        &ThemeSpec::default(),
        None,
        &OsLook::default(),
        FontSizes::default(),
        &[],
    )
});

impl Default for Theme {
    /// Fluent светлая с акцентом по умолчанию.
    fn default() -> Self {
        FALLBACK.clone()
    }
}

impl Theme {
    /// Текущая тема окна; до [`Themes::init`] — Fluent светлая.
    pub fn global(cx: &App) -> &Theme {
        cx.try_global::<Themes>().map_or(&FALLBACK, |t| t.state().theme())
    }

    /// Цвет роли.
    pub fn c(&self, r: Role) -> Hsla {
        self.colors[r as usize]
    }

    /// Метрика, логические px.
    pub fn m(&self, m: Metric) -> f32 {
        self.metrics[m as usize]
    }

    /// Метрика как `Pixels`.
    pub fn px(&self, m: Metric) -> Pixels {
        px(self.m(m))
    }

    /// Роль приложения; не объявлена — `text.primary` (и запись в журнал).
    pub fn app(&self, name: &str) -> Hsla {
        self.app_opt(name).unwrap_or_else(|| {
            tracing::debug!(target: "pets_ui::theme", role = name, "app role is not declared");
            self.c(Role::TextPrimary)
        })
    }

    pub fn app_opt(&self, name: &str) -> Option<Hsla> {
        self.app.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
    }

    /// Id выбранной темы (встроенной или из файла).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Название из файла темы; у встроенных — `None` (название — через Fluent, [`title`]).
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    pub fn is_dark(&self) -> bool {
        match self.appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::HighContrast => {
                let bg = Color::from_hsla(self.c(Role::BgBase));
                contrast_c(bg, Color::BLACK) < contrast_c(bg, Color::WHITE)
            }
        }
    }

    pub fn density(&self) -> Density {
        self.density
    }

    /// Тема целиком как файл с новым `id`: все роли варианта и плотность — для «сохранить как свою».
    pub fn to_spec(&self, id: &str) -> ThemeSpec {
        let table: std::collections::BTreeMap<String, Color> = Role::ALL
            .iter()
            .map(|r| (r.name().to_owned(), Color::from_hsla(self.c(*r))))
            .chain(self.app.iter().map(|(n, c)| ((*n).to_owned(), Color::from_hsla(*c))))
            .collect();
        let dark = self.is_dark();
        ThemeSpec {
            id: id.to_owned(),
            name: self.name.clone(),
            appearance: Some(if dark { Appearance::Dark } else { Appearance::Light }),
            accent: None,
            density: Some(self.density),
            light: if dark { Default::default() } else { table.clone() },
            dark: if dark { table } else { Default::default() },
        }
    }
}

/// Встроенная тема по id (кроме [`HIGH_CONTRAST`] — у неё своя палитра, см. [`resolve`]).
pub fn builtin(id: &str) -> Option<ThemeSpec> {
    let appearance = match id {
        SYSTEM | HIGH_CONTRAST => None,
        FLUENT_LIGHT => Some(Appearance::Light),
        FLUENT_DARK => Some(Appearance::Dark),
        _ => return None,
    };
    Some(ThemeSpec {
        id: id.to_owned(),
        appearance,
        ..ThemeSpec::default()
    })
}

/// Собрать тему: `spec` поверх Fluent, `user` (переопределения) поверх темы; см. слои в описании модуля.
pub fn resolve(spec: &ThemeSpec, user: Option<&ThemeSpec>, os: &OsLook, fonts: FontSizes, app: &[AppRole]) -> Theme {
    let density = user.and_then(|u| u.density).or(spec.density).unwrap_or_default();
    let metrics = roles::metrics(density, fonts);
    if os.high_contrast || spec.id == HIGH_CONTRAST {
        let hc = os.hc.unwrap_or_default();
        let p = builtin::high_contrast(&hc);
        return Theme {
            id: spec.id.clone(),
            name: spec.name.clone(),
            appearance: Appearance::HighContrast,
            density,
            colors: p.map(Color::hsla),
            metrics,
            app: app.iter().map(|a| (a.name, p[a.hc as usize].hsla())).collect(),
        };
    }
    let appearance = spec
        .appearance
        .unwrap_or(if os.dark { Appearance::Dark } else { Appearance::Light });
    let dark = appearance == Appearance::Dark;
    let accent = match user.and_then(|u| u.accent).or(spec.accent) {
        Some(Accent::Color(c)) => AccentPalette::from_base(c),
        Some(Accent::System) | None => os.accent,
    };
    let mut p = builtin::fluent(dark);
    apply_accent(&mut p, &accent, dark);
    let mut app_colors: Vec<(&'static str, Color)> = app
        .iter()
        .map(|a| (a.name, if dark { a.dark } else { a.light }))
        .collect();
    for layer in std::iter::once(spec).chain(user) {
        let table = if dark { &layer.dark } else { &layer.light };
        for (name, c) in table {
            if let Some(r) = Role::from_name(name) {
                p[r as usize] = *c;
            } else if let Some(slot) = app_colors.iter_mut().find(|(n, _)| n == name) {
                slot.1 = *c;
            }
        }
    }
    Theme {
        id: spec.id.clone(),
        name: spec.name.clone(),
        appearance,
        density,
        colors: p.map(Color::hsla),
        metrics,
        app: app_colors.into_iter().map(|(n, c)| (n, c.hsla())).collect(),
    }
}

/// Роли акцента из палитры (как у Урфина): заливка, текст-акцент с контрастом на `accent.subtle`.
fn apply_accent(p: &mut [Color; Role::COUNT], accent: &AccentPalette, dark: bool) {
    let (fill, text) = if dark {
        (accent.light2, accent.light3)
    } else {
        (accent.dark1, accent.dark2)
    };
    let subtle = if dark {
        accent.light2.with_alpha(0.16)
    } else {
        accent.base.with_alpha(0.12)
    };
    p[Role::AccentFill as usize] = fill;
    p[Role::AccentFillHover as usize] = fill.with_alpha(0.9);
    p[Role::AccentFillPressed as usize] = fill.with_alpha(0.8);
    p[Role::AccentOn as usize] = best_on(fill);
    p[Role::AccentSubtle as usize] = subtle;
    let marked_bg = over_c(subtle, p[Role::LayerPanel as usize]);
    p[Role::AccentText as usize] = ensure_contrast(text, marked_bg, dark);
}

/// Текст на акценте: белый или чёрный — где контраст выше (минимум ≈ 4,58 для любого цвета).
fn best_on(fill: Color) -> Color {
    let fill = fill.opaque();
    if contrast_c(Color::WHITE, fill) >= contrast_c(Color::BLACK, fill) {
        Color::WHITE
    } else {
        Color::BLACK
    }
}

/// Сдвигает `fg` к чёрному (светлая) или белому (тёмная), пока контраст с `bg` не станет ≥ [`MIN_CONTRAST`].
fn ensure_contrast(fg: Color, bg: Color, dark: bool) -> Color {
    let target = if dark { Color::WHITE } else { Color::BLACK };
    let mut c = fg;
    for step in 1..=20u8 {
        if contrast_c(c, bg) >= MIN_CONTRAST {
            break;
        }
        c = lerp(fg, target, f32::from(step) / 20.0);
    }
    c
}

#[cfg(test)]
mod tests;
