//! Корпоративная политика темы: `HKLM\Software\Policies\Pets\<App>` (компьютер) и тот же путь в HKCU
//! (пользователь; ADMX кладёт туда пользовательские политики). HKLM сильнее по каждому значению.
//!
//! | Значение | Тип | Смысл |
//! |---|---|---|
//! | `Theme` | `REG_SZ` | id темы (`system`, `fluent-light`, `fluent-dark`, `high-contrast` или своей): тема задана, выбор и переопределения пользователя не действуют |
//! | `AllowThemeChange` | `REG_DWORD` | `0` — смена темы запрещена (без `Theme` — «как в ОС»); `1` или нет значения — можно |
//!
//! Высокая контрастность ОС действует всегда: это специальные возможности, политикой не отключается.

use super::sys;

/// Путь ключа политики без корня: `Software\Policies\Pets\<App>`.
pub const POLICY_KEY: &str = r"Software\Policies\Pets";

/// Прочитанная политика приложения.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// `Theme`: тема, заданная администратором.
    pub theme: Option<String>,
    /// `AllowThemeChange`: `Some(false)` — смена запрещена.
    pub allow_change: Option<bool>,
}

impl Policy {
    /// Политика приложения `app` (имя ключа — как у профиля, например `Postman`). Чтение реестра — микросекунды.
    pub fn read(app: &str) -> Policy {
        sys::policy(app)
    }

    /// Выбор темы пользователем не действует.
    pub fn locked(&self) -> bool {
        self.theme.is_some() || self.allow_change == Some(false)
    }

    /// Тема по политике, если выбор закрыт.
    pub fn forced(&self) -> Option<&str> {
        match (&self.theme, self.allow_change) {
            (Some(t), _) => Some(t.as_str()),
            (None, Some(false)) => Some(super::SYSTEM),
            (None, _) => None,
        }
    }
}
