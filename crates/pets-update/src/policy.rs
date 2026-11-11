//! Групповая политика обновления: `HKLM\Software\Policies\Aahz\<Продукт>` (компьютер) и тот же путь в HKCU
//! (пользовательские политики ADMX). HKLM сильнее.
//!
//! | Значение | Тип | Смысл |
//! |---|---|---|
//! | `AllowUpdate` | `REG_DWORD` | `0` — обновление выключено: ни проверки, ни скачивания, сетевых запросов нет; `1` или нет значения — как в настройках |

use crate::sys;

/// Путь ключа политик издателя без корня; дальше — имя продукта.
pub const POLICY_KEY: &str = r"Software\Policies\Aahz";

/// Имя значения-выключателя.
pub const ALLOW_UPDATE: &str = "AllowUpdate";

/// Прочитанная политика продукта.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Policy {
    /// `AllowUpdate`: `Some(false)` — выключено администратором.
    pub allow_update: Option<bool>,
}

impl Policy {
    /// Политика продукта `product` (`Urfin`, `Postman`). Чтение реестра — микросекунды, сети нет.
    pub fn read(product: &str) -> Policy {
        let path = format!(r"{POLICY_KEY}\{product}");
        Policy::from_values(
            sys::policy_dword(sys::Hive::Machine, &path, ALLOW_UPDATE),
            sys::policy_dword(sys::Hive::User, &path, ALLOW_UPDATE),
        )
    }

    /// Политика из сырых значений `AllowUpdate` компьютера и пользователя.
    pub fn from_values(machine: Option<u32>, user: Option<u32>) -> Policy {
        Policy {
            allow_update: machine.or(user).map(|v| v != 0),
        }
    }

    /// Обновление не запрещено администратором.
    pub fn updates_allowed(&self) -> bool {
        self.allow_update != Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_switch() {
        assert!(Policy::from_values(None, None).updates_allowed());
        assert!(Policy::from_values(Some(1), None).updates_allowed());
        assert!(!Policy::from_values(Some(0), None).updates_allowed());
        assert!(!Policy::from_values(None, Some(0)).updates_allowed());
        // Компьютер сильнее пользователя — в обе стороны.
        assert!(!Policy::from_values(Some(0), Some(1)).updates_allowed());
        assert!(Policy::from_values(Some(1), Some(0)).updates_allowed());
    }

    /// На стенде и в CI политики нет: чтение настоящего реестра не падает и ничего не запрещает.
    #[test]
    fn policy_absent_in_registry() {
        assert_eq!(Policy::read("PetsUpdateTestNoSuchProduct"), Policy::default());
    }
}
