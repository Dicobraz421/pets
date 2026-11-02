//! Состояние тем: выбор пользователя, переопределения, темы из профиля, вид ОС, политика → итоговая тема.
//!
//! [`ThemeState`] — без gpui, проверяется тестами; [`Themes`] — `Global` gpui поверх него. Любое изменение,
//! которое меняет итоговую тему, перерисовывает все окна (`refresh_windows` обходит кэш view).

use std::path::{Path, PathBuf};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::{App, BorrowAppContext as _, Global, SharedString, Subscription, Window, WindowAppearance};
use pets_i18n::tr;

use super::spec::valid_id;
use super::sys::{self, Signal};
use super::{
    AppRole, BUILTIN, FLUENT_DARK, FLUENT_LIGHT, FontSizes, HIGH_CONTRAST, OsLook, Policy, SYSTEM, Theme, ThemeError,
    ThemeSpec, builtin, resolve,
};

/// Настройки при запуске.
#[derive(Clone, Debug)]
pub struct ThemeConfig {
    /// Имя приложения для политики (`HKLM\Software\Policies\Pets\<app>`), например `Postman`.
    pub app: &'static str,
    /// Выбор пользователя: [`SYSTEM`], id встроенной или своей темы.
    pub choice: String,
    /// Переопределения пользователя (конфиг приложения, формат темы).
    pub overrides: Option<ThemeSpec>,
    /// Папка тем профиля (`%APPDATA%\<App>\themes`): слежение и горячая перезагрузка.
    pub user_dir: Option<PathBuf>,
    /// Темы из `user_dir`, прочитанные приложением при запуске ([`load_dir`]) — до первого кадра.
    pub user_themes: Vec<ThemeSpec>,
    /// Роли приложения (`app.*`).
    pub app_roles: &'static [AppRole],
    pub fonts: FontSizes,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            app: "Pets",
            choice: SYSTEM.to_owned(),
            overrides: None,
            user_dir: None,
            user_themes: Vec::new(),
            app_roles: &[],
            fonts: FontSizes::default(),
        }
    }
}

/// Выбор, слои и итоговая тема.
#[derive(Clone, Debug)]
pub struct ThemeState {
    choice: String,
    overrides: Option<ThemeSpec>,
    user: Vec<ThemeSpec>,
    os: OsLook,
    policy: Policy,
    fonts: FontSizes,
    app_roles: &'static [AppRole],
    current: Theme,
}

impl ThemeState {
    pub fn new(cfg: ThemeConfig, os: OsLook, policy: Policy) -> Self {
        let mut s = Self {
            choice: cfg.choice,
            overrides: cfg.overrides,
            user: Vec::new(),
            os,
            policy,
            fonts: cfg.fonts,
            app_roles: cfg.app_roles,
            current: Theme::default(),
        };
        s.user = keep_valid(cfg.user_themes);
        s.recompute();
        s
    }

    pub fn theme(&self) -> &Theme {
        &self.current
    }

    /// Выбор пользователя (как сохранить в конфиг).
    pub fn choice(&self) -> &str {
        &self.choice
    }

    /// Тема, которая действует: политика сильнее выбора; неизвестная — «как в ОС».
    pub fn effective(&self) -> &str {
        let id = self.policy.forced().unwrap_or(&self.choice);
        if self.find(id).is_some() { id } else { SYSTEM }
    }

    /// Смену темы запретила политика: выбор в интерфейсе — только для чтения.
    pub fn locked(&self) -> bool {
        self.policy.locked()
    }

    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    pub fn os(&self) -> &OsLook {
        &self.os
    }

    pub fn app_roles(&self) -> &'static [AppRole] {
        self.app_roles
    }

    /// Темы для списка: встроенные, затем свои (по id).
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        BUILTIN.iter().copied().chain(self.user.iter().map(|t| t.id.as_str()))
    }

    /// Выбрать тему. `Ok(true)` — итоговая тема изменилась.
    pub fn set_choice(&mut self, id: &str) -> Result<bool, ThemeError> {
        if self.locked() {
            return Err(ThemeError::Locked);
        }
        if self.find(id).is_none() {
            return Err(ThemeError::Unknown(id.to_owned()));
        }
        id.clone_into(&mut self.choice);
        Ok(self.recompute())
    }

    /// Переопределения пользователя (плотность, акцент, роли). При политике — сохраняются, но не действуют.
    pub fn set_overrides(&mut self, overrides: Option<ThemeSpec>) -> bool {
        self.overrides = overrides;
        self.recompute()
    }

    /// Плотность — в слое пользователя, действует и при политике.
    pub fn set_density(&mut self, density: Option<super::Density>) -> bool {
        self.overrides.get_or_insert_with(ThemeSpec::default).density = density;
        self.recompute()
    }

    pub fn set_fonts(&mut self, fonts: FontSizes) -> bool {
        self.fonts = fonts;
        self.recompute()
    }

    pub fn set_os(&mut self, os: OsLook) -> bool {
        self.os = os;
        self.recompute()
    }

    /// Окно сообщило вид (светлая/тёмная) раньше, чем поток слежения перечитал реестр.
    pub fn set_os_dark(&mut self, dark: bool) -> bool {
        self.os.dark = dark;
        self.recompute()
    }

    pub fn set_user_themes(&mut self, themes: Vec<ThemeSpec>) -> bool {
        self.user = keep_valid(themes);
        self.recompute()
    }

    fn find(&self, id: &str) -> Option<ThemeSpec> {
        builtin(id).or_else(|| self.user.iter().find(|t| t.id == id).cloned())
    }

    /// Пересчитать итоговую тему; `true` — изменилась.
    fn recompute(&mut self) -> bool {
        let spec = self.find(self.effective()).unwrap_or_default();
        let user = match (&self.overrides, self.locked()) {
            (Some(o), false) => Some(o.clone()),
            // Политика: свои цвета и акцент не действуют, плотность — да.
            (Some(o), true) => Some(ThemeSpec {
                density: o.density,
                ..ThemeSpec::default()
            }),
            (None, _) => None,
        };
        let next = resolve(&spec, user.as_ref(), &self.os, self.fonts, self.app_roles);
        let changed = next != self.current;
        self.current = next;
        changed
    }
}

fn keep_valid(mut themes: Vec<ThemeSpec>) -> Vec<ThemeSpec> {
    themes.retain(|t| valid_id(&t.id) && builtin(&t.id).is_none());
    themes.sort_by(|a, b| a.id.cmp(&b.id));
    themes.dedup_by(|a, b| a.id == b.id);
    themes
}

/// Прочитать темы из папки (`*.toml`, id — имя файла). Это файловый ввод-вывод: звать при чтении настроек
/// до окна или в фоне, не в `render`. Ошибки — по файлу, остальные темы читаются.
pub fn load_dir(dir: &Path, app: &[AppRole]) -> (Vec<ThemeSpec>, Vec<ThemeError>) {
    let mut themes = Vec::new();
    let mut errors = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (themes, errors),
        Err(e) => {
            errors.push(ThemeError::Io {
                path: dir.display().to_string(),
                msg: e.to_string(),
            });
            return (themes, errors);
        }
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")))
        .collect();
    paths.sort();
    for path in paths {
        let id = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                errors.push(ThemeError::Io {
                    path: path.display().to_string(),
                    msg: e.to_string(),
                });
                continue;
            }
        };
        match ThemeSpec::parse(&id, &text, app) {
            Ok(t) if builtin(&t.id).is_some() => errors.push(ThemeError::Reserved(t.id)),
            Ok(t) if !valid_id(&t.id) => errors.push(ThemeError::Invalid {
                id: t.id,
                issues: vec![super::Issue::BadValue {
                    key: "id",
                    value: "file name: want [a-z0-9-]".into(),
                }],
            }),
            Ok(t) => themes.push(t),
            Err(e) => errors.push(e),
        }
    }
    (themes, errors)
}

/// Название темы для списка: свои — из файла (`name`, иначе id), встроенные — через Fluent.
pub fn title(id: &str, state: &ThemeState) -> SharedString {
    let text = match id {
        SYSTEM => tr!("pets-ui-theme-system"),
        FLUENT_LIGHT => tr!("pets-ui-theme-fluent-light"),
        FLUENT_DARK => tr!("pets-ui-theme-fluent-dark"),
        HIGH_CONTRAST => tr!("pets-ui-theme-high-contrast"),
        _ => state
            .user
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.name.clone())
            .unwrap_or_else(|| id.to_owned()),
    };
    SharedString::from(text)
}

/// Темы окна приложения — gpui `Global`. Наблюдать: `cx.observe_global::<Themes>(…)`.
pub struct Themes {
    state: ThemeState,
    /// Ошибки последнего чтения папки тем (для тоста «тема с ошибками»).
    errors: Vec<ThemeError>,
}

impl Global for Themes {}

/// Событие потока слежения.
enum Change {
    Os(OsLook),
    Themes(Vec<ThemeSpec>, Vec<ThemeError>),
}

impl Themes {
    /// Прочитать вид ОС и политику (реестр, микросекунды), поставить `Global`, запустить слежение за ОС и
    /// папкой тем. Звать один раз до первого окна.
    pub fn init(cfg: ThemeConfig, cx: &mut App) {
        let os = OsLook::read();
        let policy = Policy::read(cfg.app);
        if policy.locked() {
            tracing::info!(target: "pets_ui::theme", ?policy, "theme is set by policy");
        }
        let dir = cfg.user_dir.clone();
        let app_roles = cfg.app_roles;
        let state = ThemeState::new(cfg, os, policy);
        tracing::debug!(target: "pets_ui::theme", theme = state.effective(), ?os, "theme");
        cx.set_global(Themes {
            state,
            errors: Vec::new(),
        });
        let (tx, mut rx) = mpsc::unbounded::<Change>();
        let mut last = os;
        sys::watch(
            dir.clone(),
            Box::new(move |signal| match signal {
                Signal::Look => {
                    let now = OsLook::read();
                    if now == last {
                        return true;
                    }
                    last = now;
                    tracing::info!(target: "pets_ui::theme", ?now, "system look changed");
                    tx.unbounded_send(Change::Os(now)).is_ok()
                }
                Signal::Dir => {
                    let Some(dir) = &dir else { return true };
                    let (themes, errors) = load_dir(dir, app_roles);
                    tracing::info!(target: "pets_ui::theme", themes = themes.len(), errors = errors.len(), "themes reloaded");
                    tx.unbounded_send(Change::Themes(themes, errors)).is_ok()
                }
            }),
        );
        cx.spawn(async move |cx| {
            while let Some(change) = rx.next().await {
                cx.update(|cx| {
                    Themes::update_all(cx, |t| match change {
                        Change::Os(os) => t.state.set_os(os),
                        Change::Themes(themes, errors) => {
                            for e in &errors {
                                tracing::warn!(target: "pets_ui::theme", "{e}");
                            }
                            t.errors = errors;
                            t.state.set_user_themes(themes)
                        }
                    });
                });
            }
        })
        .detach();
    }

    /// Состояние; до [`Themes::init`] — `None`.
    pub fn try_get(cx: &App) -> Option<&Themes> {
        cx.try_global::<Themes>()
    }

    pub fn state(&self) -> &ThemeState {
        &self.state
    }

    /// Ошибки файлов тем с последнего чтения папки.
    pub fn errors(&self) -> &[ThemeError] {
        &self.errors
    }

    /// Изменить состояние (`f` → «тема изменилась»): изменилась — перерисовать окна. Без [`Themes::init`] —
    /// ничего, `false`.
    pub fn update(cx: &mut App, f: impl FnOnce(&mut ThemeState) -> bool) -> bool {
        Themes::update_all(cx, |t| f(&mut t.state))
    }

    /// Наблюдатели `observe_global` узнают о любом вызове (и об ошибках файлов тем), окна перерисовываются,
    /// только если тема изменилась.
    fn update_all(cx: &mut App, f: impl FnOnce(&mut Themes) -> bool) -> bool {
        if !cx.has_global::<Themes>() {
            return false;
        }
        let changed = cx.update_global::<Themes, _>(|t, _| f(t));
        if changed {
            cx.refresh_windows();
        }
        changed
    }

    /// Выбор пользователя; ошибка — неизвестная тема или запрет политики.
    pub fn set_choice(id: &str, cx: &mut App) -> Result<(), ThemeError> {
        let mut result = Ok(());
        Themes::update(cx, |s| match s.set_choice(id) {
            Ok(changed) => changed,
            Err(e) => {
                result = Err(e);
                false
            }
        });
        result
    }

    /// Следить за видом окна gpui (светлая ↔ тёмная): тема меняется сразу, акцент дочитает поток слежения.
    /// Подписку держать, пока живо окно (или `detach`).
    pub fn observe_window(window: &Window) -> Subscription {
        window.observe_window_appearance(|window, cx| {
            let dark = matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            );
            Themes::update(cx, |s| s.set_os_dark(dark));
        })
    }
}
