//! Описание продукта, канал, настройки пользователя, вид установки.

use std::path::Path;

use semver::Version;
use serde::{Deserialize, Serialize};

/// ОС в имени артефакта.
pub const OS: &str = "windows";

/// Архитектура в имени артефакта: `x64` или `arm64`.
pub const ARCH: &str = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };

/// Продукт, который обновляется. Задаётся в коде продукта константами.
#[derive(Debug, Clone)]
pub struct Product {
    /// Имя для политики (`HKLM\Software\Policies\Aahz\<name>`), например `Urfin`.
    pub name: &'static str,
    /// Репозиторий выпусков на GitHub: `владелец/имя`.
    pub repo: &'static str,
    /// Начало имён артефактов (нижний регистр): `urfin` → `urfin-<версия>-windows-x64.msi`.
    pub asset_prefix: &'static str,
    /// Версия запущенной программы.
    pub current: Version,
    /// Открытые ключи minisign (строка base64 из `.pub`). Несколько — на время смены ключа.
    pub public_keys: &'static [&'static str],
}

/// Канал обновлений.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// Только стабильные выпуски (версия без дефиса).
    #[default]
    Stable,
    /// Стабильные и предварительные (`-beta.N`, `-rc.N`).
    Beta,
}

/// Настройки пользователя; хранит их продукт.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Проверять раз в сутки. Выключено — запросов по расписанию нет, остаётся кнопка «Проверить».
    pub enabled: bool,
    /// Канал.
    pub channel: Channel,
}

impl Default for Settings {
    /// Включено, стабильный канал (решение владельца 2026-11-11).
    fn default() -> Self {
        Settings {
            enabled: true,
            channel: Channel::Stable,
        }
    }
}

/// Как установлена программа — какой артефакт скачивать.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallKind {
    /// Установлена пакетом MSI: скачивается `.msi`, ставит `msiexec`.
    Msi,
    /// Portable: скачивается `.zip`, файлы заменяются рядом с exe.
    Portable,
}

impl InstallKind {
    /// Portable, если рядом с exe лежит файл-флаг (`urfin.portable`), иначе MSI.
    pub fn detect(exe_dir: &Path, portable_flag: &str) -> InstallKind {
        if exe_dir.join(portable_flag).exists() {
            InstallKind::Portable
        } else {
            InstallKind::Msi
        }
    }

    /// Расширение артефакта без точки.
    pub fn extension(self) -> &'static str {
        match self {
            InstallKind::Msi => "msi",
            InstallKind::Portable => "zip",
        }
    }
}

impl Product {
    /// Имя артефакта выпуска: `<prefix>-<версия>-windows-<arch>.<msi|zip>`.
    pub fn asset_name(&self, version: &Version, kind: InstallKind) -> String {
        format!("{}-{version}-{OS}-{ARCH}.{}", self.asset_prefix, kind.extension())
    }
}
