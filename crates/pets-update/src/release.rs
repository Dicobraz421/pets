//! Разбор ответа GitHub Releases и выбор выпуска: канал, semver, артефакт и его подпись.

use semver::Version;
use serde::Deserialize;

use crate::{Channel, InstallKind, Product, UpdateError};

/// Расширение файла подписи рядом с артефактом: `<артефакт>.minisig`.
pub const SIGNATURE_EXT: &str = "minisig";

/// Найденное обновление: выпуск новее текущей версии, с артефактом нужного вида и подписью.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    /// Версия выпуска.
    pub version: Version,
    /// Вид артефакта.
    pub kind: InstallKind,
    /// Страница выпуска — для ссылки «Что нового».
    pub page_url: String,
    /// Имя артефакта: `<prefix>-<версия>-windows-<arch>.<msi|zip>`.
    pub asset_name: String,
    /// Адрес артефакта.
    pub asset_url: String,
    /// Размер артефакта в байтах.
    pub size: u64,
    /// Адрес подписи `<артефакт>.minisig`.
    pub signature_url: String,
}

/// Адрес списка выпусков репозитория `owner/name`. В запросе — только имя репозитория.
pub fn releases_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases?per_page=30")
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Выбрать из ответа `GET /repos/<repo>/releases` самый новый подходящий выпуск.
///
/// Подходит выпуск: не черновик; тег `v<semver>`; версия строго больше текущей (откат не предлагается); на
/// стабильном канале — без предварительной метки; есть артефакт вида `kind` и его подпись, оба по HTTPS.
/// Выпуск без артефакта или подписи (ещё публикуется) пропускается — берётся следующий по старшинству.
pub fn select(
    json: &str,
    product: &Product,
    channel: Channel,
    kind: InstallKind,
) -> Result<Option<Update>, UpdateError> {
    let releases: Vec<GhRelease> = serde_json::from_str(json)?;
    let mut best: Option<Update> = None;
    for release in releases {
        if release.draft {
            continue;
        }
        let tag = release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name);
        let Ok(version) = Version::parse(tag) else {
            tracing::debug!(tag = %release.tag_name, "update: tag is not a version, skipped");
            continue;
        };
        let pre = release.prerelease || !version.pre.is_empty();
        if (pre && channel == Channel::Stable) || version <= product.current {
            continue;
        }
        if best.as_ref().is_some_and(|b| b.version >= version) {
            continue;
        }
        let asset_name = product.asset_name(&version, kind);
        let signature_name = format!("{asset_name}.{SIGNATURE_EXT}");
        let find = |name: &str| {
            release
                .assets
                .iter()
                .find(|a| a.name == name && a.browser_download_url.starts_with("https://"))
        };
        let (Some(asset), Some(signature)) = (find(&asset_name), find(&signature_name)) else {
            tracing::warn!(%version, asset = %asset_name, "update: release has no artifact or signature, skipped");
            continue;
        };
        best = Some(Update {
            version,
            kind,
            page_url: release.html_url.clone(),
            asset_url: asset.browser_download_url.clone(),
            size: asset.size,
            signature_url: signature.browser_download_url.clone(),
            asset_name,
        });
    }
    Ok(best)
}
