//! Визуальный язык приложений Pets на gpui: единый стиль всех приложений (базис Pets).
//!
//! - [`theme`] — темы оформления: цвета ролей ([`theme::Role`]), метрики и плотность ([`theme::Metric`],
//!   [`theme::Density`]), режим «как в ОС», темы в TOML с горячей перезагрузкой, корпоративная политика;
//!   в gpui — `Global` [`theme::Themes`], цвет — `Theme::global(cx).c(Role::TextPrimary)`.
//! - [`Toasts`] — тосты справа сверху: не забирают фокус, исчезают сами (ошибки дольше), щелчок — закрыть,
//!   кнопка действия («Отменить»); история уведомлений — панель справа ([`Toasts::toggle_history`]).
//!
//! Тексты — через `pets-i18n`: приложение добавляет [`FTL`] к своим наборам.

pub mod theme;
mod toasts;

pub use theme::Theme;
pub use toasts::{ACTION_DURATION, ActionFn, Level, Queue, Toast, ToastAction, Toasts};

/// Тексты крейта (ключи `pets-ui-…`), RU и EN.
pub const FTL: &[pets_i18n::Resource] = &[
    ("ru", include_str!("../i18n/ru.ftl")),
    ("en", include_str!("../i18n/en.ftl")),
];
