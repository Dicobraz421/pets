//! Описание продукта, канал, настройки пользователя, вид установки.

use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

/// ОС в имени артефакта: `windows`, `macos` или `linux`.
pub const OS: &str = if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(target_os = "linux") {
    "linux"
} else {
    "windows"
};

/// Архитектура в имени артефакта: `x64` или `arm64`.
pub const ARCH: &str = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };

/// Продукт, который обновляется. Задаётся в коде продукта константами.
#[derive(Debug, Clone)]
pub struct Product {
    /// Имя для политики (`HKLM\Software\Policies\Aahz\<name>`), например `Urfin`.
    pub name: &'static str,
    /// Репозиторий выпусков на GitHub: `владелец/имя`.
    pub repo: &'static str,
    /// Начало имён артефактов (нижний регистр): `urfin` → `urfin-<версия>-windows-x64.msi`,
    /// `postman` → `postman-<версия>-macos-arm64.zip`.
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
    /// macOS: программа — бандл `<Имя>.app` ([`app_bundle`]); скачивается `.zip` с ним (`ditto`, рядом может лежать
    /// `README.txt`), бандл заменяется целиком с откатом.
    AppBundle,
    /// Linux: программа — один файл `.AppImage` (`$APPIMAGE`); скачивается новый `.AppImage` и атомарно заменяет
    /// старый переименованием.
    #[serde(rename = "appimage")]
    AppImage,
    /// Linux: пакет `.deb`. Ставит менеджер пакетов от root — крейт не устанавливает, продукт даёт ссылку «Скачать»
    /// ([`Update::asset_url`](crate::Update::asset_url)); [`can_install`](Self::can_install) — `false`.
    Deb,
    /// Linux: пакет `.rpm` — как [`Deb`](Self::Deb).
    Rpm,
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

    /// Вид установки на Linux: задан `$APPIMAGE` (`appimage`) — [`AppImage`](Self::AppImage); exe под `<root>/usr` —
    /// пакет: [`Deb`](Self::Deb), если в системе есть `dpkg`, иначе [`Rpm`](Self::Rpm), если есть `rpm`. Иначе
    /// (сборка из исходников, неизвестная система пакетов) — `None`: обновлять нечем. `root` — корень ФС (`/`).
    pub fn detect_linux(appimage: Option<&Path>, exe: &Path, root: &Path) -> Option<InstallKind> {
        if appimage.is_some_and(|p| !p.as_os_str().is_empty()) {
            return Some(InstallKind::AppImage);
        }
        if !exe.starts_with(root.join("usr")) {
            return None;
        }
        if root.join("usr/bin/dpkg").exists() {
            Some(InstallKind::Deb)
        } else if root.join("usr/bin/rpm").exists() {
            Some(InstallKind::Rpm)
        } else {
            None
        }
    }

    /// [`detect_linux`](Self::detect_linux) для запущенной программы: `$APPIMAGE`, свой exe, корень `/`.
    pub fn current_linux() -> Option<InstallKind> {
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
        let exe = std::env::current_exe().ok()?;
        let kind = InstallKind::detect_linux(appimage.as_deref(), &exe, Path::new("/"));
        tracing::debug!(?kind, "update: install kind detected");
        kind
    }

    /// Может ли крейт сам поставить обновление. `false` — пакеты deb/rpm: продукт показывает «Доступна версия X» и
    /// ссылку «Скачать» на [`Update::asset_url`](crate::Update::asset_url).
    pub fn can_install(self) -> bool {
        !matches!(self, InstallKind::Deb | InstallKind::Rpm)
    }

    /// Расширение артефакта без точки.
    pub fn extension(self) -> &'static str {
        match self {
            InstallKind::Msi => "msi",
            InstallKind::Portable | InstallKind::AppBundle => "zip",
            InstallKind::AppImage => "AppImage",
            InstallKind::Deb => "deb",
            InstallKind::Rpm => "rpm",
        }
    }
}

/// Бандл `.app`, в котором лежит exe: `…/Имя.app/Contents/MacOS/<exe>` → `…/Имя.app`. Не в бандле — `None`.
pub fn app_bundle(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_app = bundle.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && is_app).then(|| bundle.to_path_buf())
}

impl Product {
    /// Имя артефакта выпуска: `<prefix>-<версия>-<os>-<arch>.<msi|zip|AppImage|deb|rpm>` ([`OS`], [`ARCH`]).
    pub fn asset_name(&self, version: &Version, kind: InstallKind) -> String {
        format!("{}-{version}-{OS}-{ARCH}.{}", self.asset_prefix, kind.extension())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_kind_is_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let exe = root.join("usr/bin/postman");
        let home = root.join("home/u/postman");
        let appimage = root.join("home/u/Postman.AppImage");
        assert_eq!(
            InstallKind::detect_linux(Some(&appimage), &exe, root),
            Some(InstallKind::AppImage)
        );
        assert_eq!(InstallKind::detect_linux(Some(Path::new("")), &home, root), None);
        assert_eq!(InstallKind::detect_linux(None, &exe, root), None);
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        std::fs::write(root.join("usr/bin/rpm"), b"").unwrap();
        assert_eq!(InstallKind::detect_linux(None, &exe, root), Some(InstallKind::Rpm));
        std::fs::write(root.join("usr/bin/dpkg"), b"").unwrap();
        assert_eq!(InstallKind::detect_linux(None, &exe, root), Some(InstallKind::Deb));
        assert_eq!(InstallKind::detect_linux(None, &home, root), None);
    }

    #[test]
    fn linux_asset_names() {
        let product = Product {
            name: "Postman",
            repo: "owner/postman",
            asset_prefix: "postman",
            current: Version::new(1, 0, 0),
            public_keys: &[],
        };
        let v = Version::new(1, 2, 0);
        for (kind, ext, installs) in [
            (InstallKind::AppImage, "AppImage", true),
            (InstallKind::Deb, "deb", false),
            (InstallKind::Rpm, "rpm", false),
        ] {
            assert_eq!(product.asset_name(&v, kind), format!("postman-1.2.0-{OS}-{ARCH}.{ext}"));
            assert_eq!(kind.can_install(), installs);
        }
        assert_eq!(serde_json::to_string(&InstallKind::AppImage).unwrap(), "\"appimage\"");
    }
}
