//! Сценарии обновления без сети: разбор ответа GitHub по фикстуре, канал, откат версии, выключатели, расписание,
//! скачивание с прогрессом и отменой, подмена файла и подписи.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pets_update::{
    ARCH, Cancel, Channel, CheckOutcome, CheckReason, DisabledBy, InstallKind, OS, Policy, Product, Progress, Response,
    Settings, SignatureError, Transport, Update, UpdateError, Updater, Version, releases_url, select,
};

/// Ответ GitHub; имена артефактов в нём — для этой ОС и архитектуры (в файле — `windows-x64`).
fn fixture() -> String {
    include_str!("fixtures/releases.json").replace("-windows-x64.", &format!("-{OS}-{ARCH}."))
}

/// Имя артефакта этой ОС и архитектуры: `asset("app-0.2.0", "msi")` → `app-0.2.0-windows-x64.msi`.
fn asset(stem: &str, ext: &str) -> String {
    format!("{stem}-{OS}-{ARCH}.{ext}")
}
const REPO: &str = "Aahz/app";
/// Содержимое «установщика»: 16 байт, как `size` в фикстуре.
const INSTALLER: &[u8] = b"0123456789abcdef";

/// Подменная сеть: ответы по адресам и журнал запросов.
#[derive(Clone, Default)]
struct Net {
    routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    calls: Arc<Mutex<Vec<String>>>,
    /// Отменить этим токеном после первой порции тела.
    cancel_on_read: Option<Cancel>,
}

impl Net {
    fn with_releases() -> Net {
        let net = Net::default();
        net.route(&releases_url(REPO), fixture().as_bytes());
        net
    }

    fn route(&self, url: &str, body: &[u8]) {
        self.routes.lock().unwrap().insert(url.to_owned(), body.to_vec());
    }

    fn calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

struct CancelAfterFirst(Cursor<Vec<u8>>, Cancel);

impl Read for CancelAfterFirst {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.0.read(&mut buf[..4])?;
        self.1.cancel();
        Ok(n)
    }
}

impl Transport for Net {
    fn get(&self, url: &str) -> Result<Response, UpdateError> {
        self.calls.lock().unwrap().push(url.to_owned());
        let body = self
            .routes
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .ok_or(UpdateError::Http(404))?;
        let len = Some(body.len() as u64);
        let body: Box<dyn Read + Send> = match &self.cancel_on_read {
            Some(cancel) if !url.ends_with(".minisig") => Box::new(CancelAfterFirst(Cursor::new(body), cancel.clone())),
            _ => Box::new(Cursor::new(body)),
        };
        Ok(Response { len, body })
    }
}

fn product(current: &str, keys: &'static [&'static str]) -> Product {
    Product {
        name: "App",
        repo: REPO,
        asset_prefix: "app",
        current: Version::parse(current).unwrap(),
        public_keys: keys,
    }
}

fn updater(current: &str, dir: &Path, policy: Policy, net: &Net) -> Updater {
    Updater::with_parts(
        product(current, &[]),
        InstallKind::Msi,
        dir,
        policy,
        Box::new(net.clone()),
    )
}

fn pick(current: &str, channel: Channel, kind: InstallKind) -> Option<Update> {
    select(&fixture(), &product(current, &[]), channel, kind).unwrap()
}

fn version(update: Option<Update>) -> Option<String> {
    update.map(|u| u.version.to_string())
}

const BETA: Settings = Settings {
    enabled: true,
    channel: Channel::Beta,
};
const OFF: Settings = Settings {
    enabled: false,
    channel: Channel::Stable,
};

// ── Разбор ответа GitHub, канал, откат ──────────────────────────────────────────────────────────────────────────

#[test]
fn github_fixture_is_parsed() {
    let update = pick("0.1.0", Channel::Stable, InstallKind::Msi).unwrap();
    assert_eq!(update.version, Version::new(0, 2, 0));
    assert_eq!(update.kind, InstallKind::Msi);
    assert_eq!(update.asset_name, asset("app-0.2.0", "msi"));
    assert_eq!(update.size, 16);
    assert_eq!(
        update.asset_url,
        format!(
            "https://github.com/Aahz/app/releases/download/v0.2.0/{}",
            asset("app-0.2.0", "msi")
        )
    );
    assert_eq!(update.signature_url, format!("{}.minisig", update.asset_url));
    assert_eq!(update.page_url, "https://github.com/Aahz/app/releases/tag/v0.2.0");

    let portable = pick("0.1.0", Channel::Stable, InstallKind::Portable).unwrap();
    assert_eq!(portable.asset_name, asset("app-0.2.0", "zip"));

    assert!(matches!(
        select(
            "{\"message\":\"rate limit\"}",
            &product("0.1.0", &[]),
            Channel::Stable,
            InstallKind::Msi
        ),
        Err(UpdateError::Parse(_))
    ));
}

#[test]
fn channel_selects_releases() {
    // Стабильный канал не видит предварительных выпусков.
    assert_eq!(
        version(pick("0.1.0", Channel::Stable, InstallKind::Msi)).as_deref(),
        Some("0.2.0")
    );
    assert_eq!(version(pick("0.2.0", Channel::Stable, InstallKind::Msi)), None);
    // Бета: 0.3.0-beta.2 ещё без подписи — берётся beta.1; черновик 0.4.0 и тег `nightly` не считаются.
    assert_eq!(
        version(pick("0.1.0", Channel::Beta, InstallKind::Msi)).as_deref(),
        Some("0.3.0-beta.1")
    );
    // У 0.1.0 и beta.2 нет zip: portable получает то, что для него выложено.
    assert_eq!(
        version(pick("0.2.0", Channel::Beta, InstallKind::Portable)).as_deref(),
        Some("0.3.0-beta.1")
    );
    // Бета-версия на стабильном канале дожидается стабильного выпуска той же версии.
    assert_eq!(
        version(pick("0.2.0-rc.1", Channel::Stable, InstallKind::Msi)).as_deref(),
        Some("0.2.0")
    );
}

#[test]
fn downgrade_is_never_offered() {
    for channel in [Channel::Stable, Channel::Beta] {
        // Та же версия и версии новее всего выложенного: предлагать нечего.
        assert_eq!(version(pick("0.3.0-beta.1", channel, InstallKind::Msi)), None);
        assert_eq!(version(pick("0.3.0", channel, InstallKind::Msi)), None);
        assert_eq!(version(pick("1.0.0", channel, InstallKind::Msi)), None);
    }
    // Ушёл с беты на стабильный канал: стабильная 0.2.0 старше установленной — не предлагается.
    assert_eq!(version(pick("0.3.0-beta.1", Channel::Stable, InstallKind::Msi)), None);
}

// ── Выключатели и расписание ────────────────────────────────────────────────────────────────────────────────────

#[test]
fn setting_off_means_no_background_requests() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Net::with_releases();
    let updater = updater("0.1.0", tmp.path(), Policy::default(), &net);

    let outcome = updater.check(&OFF, CheckReason::Scheduled).unwrap();
    assert_eq!(outcome, CheckOutcome::Disabled(DisabledBy::Settings));
    assert_eq!(net.calls(), 0);

    // Кнопка «Проверить» остаётся.
    let outcome = updater.check(&OFF, CheckReason::Manual).unwrap();
    assert!(matches!(outcome, CheckOutcome::Available(u) if u.version == Version::new(0, 2, 0)));
    assert_eq!(net.calls(), 1);
}

/// Канал из настроек доходит до выбора выпуска.
#[test]
fn beta_setting_offers_prerelease() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Net::with_releases();
    let updater = updater("0.2.0", tmp.path(), Policy::default(), &net);
    assert_eq!(
        updater.check(&Settings::default(), CheckReason::Manual).unwrap(),
        CheckOutcome::UpToDate
    );
    let outcome = updater.check(&BETA, CheckReason::Manual).unwrap();
    assert!(matches!(outcome, CheckOutcome::Available(u) if u.version.to_string() == "0.3.0-beta.1"));
}

#[test]
fn policy_off_means_no_requests_at_all() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Net::with_releases();
    let policy = Policy::from_values(Some(0), None);
    let updater = updater("0.1.0", tmp.path(), policy, &net);

    for reason in [CheckReason::Scheduled, CheckReason::Manual] {
        let outcome = updater.check(&Settings::default(), reason).unwrap();
        assert_eq!(outcome, CheckOutcome::Disabled(DisabledBy::Policy));
    }
    let update = pick("0.1.0", Channel::Stable, InstallKind::Msi).unwrap();
    let err = updater.download(&update, &Cancel::new(), &mut |_| {}).unwrap_err();
    assert!(matches!(err, UpdateError::DisabledByPolicy), "{err}");
    assert_eq!(net.calls(), 0);
    assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
}

#[test]
fn scheduled_check_runs_once_a_day() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Net::with_releases();
    let updater = updater("0.2.0", tmp.path(), Policy::default(), &net);
    let settings = Settings::default();
    let start = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let at = |hours: u64| start + Duration::from_secs(hours * 3600);
    let scheduled = |when: SystemTime| updater.check_at(&settings, CheckReason::Scheduled, when).unwrap();

    assert_eq!(scheduled(at(0)), CheckOutcome::UpToDate);
    assert_eq!(scheduled(at(1)), CheckOutcome::NotDue);
    assert_eq!(scheduled(at(23)), CheckOutcome::NotDue);
    assert_eq!(net.calls(), 1);
    // По кнопке — сразу, не дожидаясь суток.
    assert_eq!(
        updater.check_at(&settings, CheckReason::Manual, at(2)).unwrap(),
        CheckOutcome::UpToDate
    );
    assert_eq!(net.calls(), 2);
    assert_eq!(scheduled(at(25)), CheckOutcome::NotDue);
    assert_eq!(scheduled(at(26)), CheckOutcome::UpToDate);
    assert_eq!(net.calls(), 3);
    // Время проверки переживает перезапуск программы.
    let again = self::updater("0.2.0", tmp.path(), Policy::default(), &net);
    assert_eq!(
        again.check_at(&settings, CheckReason::Scheduled, at(27)).unwrap(),
        CheckOutcome::NotDue
    );
    assert_eq!(net.calls(), 3);
}

#[test]
fn failed_check_is_retried_in_an_hour() {
    let tmp = tempfile::tempdir().unwrap();
    let net = Net::default();
    let updater = updater("0.1.0", tmp.path(), Policy::default(), &net);
    let start = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let at = |minutes: u64| start + Duration::from_secs(minutes * 60);
    let scheduled = |when: SystemTime| updater.check_at(&Settings::default(), CheckReason::Scheduled, when);

    assert!(matches!(scheduled(at(0)), Err(UpdateError::Http(404))));
    assert_eq!(scheduled(at(30)).unwrap(), CheckOutcome::NotDue);
    assert_eq!(net.calls(), 1);
    net.route(&releases_url(REPO), fixture().as_bytes());
    assert!(matches!(scheduled(at(61)).unwrap(), CheckOutcome::Available(_)));
    assert_eq!(net.calls(), 2);
}

// ── Скачивание и подпись ────────────────────────────────────────────────────────────────────────────────────────

struct Signer {
    pair: minisign::KeyPair,
}

impl Signer {
    fn new() -> Signer {
        Signer {
            pair: minisign::KeyPair::generate_unencrypted_keypair().unwrap(),
        }
    }

    /// Открытый ключ в виде, в каком он вшит в продукт.
    fn keys(&self) -> &'static [&'static str] {
        let key: &'static str = Box::leak(self.pair.pk.to_base64().into_boxed_str());
        Box::leak(vec![key].into_boxed_slice())
    }

    /// Подпись как у `minisign -S`: имя файла — в подписанном комментарии.
    fn sign(&self, data: &[u8], file_name: &str) -> String {
        let comment = format!("timestamp:1800000000\tfile:{file_name}\thashed");
        minisign::sign(None, &self.pair.sk, Cursor::new(data), Some(&comment), None)
            .unwrap()
            .into_string()
    }
}

/// Обновление 0.1.0 → 0.2.0 с заданными телом установщика и подписью на сервере.
fn staged(dir: &Path, keys: &'static [&'static str], installer: &[u8], signature: &str) -> (Updater, Update, Net) {
    let net = Net::with_releases();
    let updater = Updater::with_parts(
        product("0.1.0", keys),
        InstallKind::Msi,
        dir,
        Policy::default(),
        Box::new(net.clone()),
    );
    let CheckOutcome::Available(update) = updater.check(&Settings::default(), CheckReason::Manual).unwrap() else {
        panic!("update expected");
    };
    net.route(&update.asset_url, installer);
    net.route(&update.signature_url, signature.as_bytes());
    (updater, update, net)
}

#[test]
fn download_reports_progress_and_verifies() {
    let tmp = tempfile::tempdir().unwrap();
    let signer = Signer::new();
    let signature = signer.sign(INSTALLER, &asset("app-0.2.0", "msi"));
    let (updater, update, _net) = staged(tmp.path(), signer.keys(), INSTALLER, &signature);

    let mut seen = Vec::new();
    let downloaded = updater
        .download(&update, &Cancel::new(), &mut |p| seen.push(p))
        .unwrap();
    assert_eq!(
        seen.last(),
        Some(&Progress {
            received: 16,
            total: 16
        })
    );
    assert_eq!(downloaded.file, tmp.path().join(asset("app-0.2.0", "msi")));
    assert_eq!(std::fs::read(&downloaded.file).unwrap(), INSTALLER);

    let verified = updater.verify(downloaded.clone()).unwrap();
    assert_eq!(verified.update(), &update);
    // Проверенный файл закрыт от подмены, пока его не отдали установке.
    #[cfg(windows)]
    assert!(std::fs::write(&downloaded.file, b"evil").is_err());
}

#[test]
fn download_can_be_cancelled() {
    let tmp = tempfile::tempdir().unwrap();
    let signer = Signer::new();
    let signature = signer.sign(INSTALLER, &asset("app-0.2.0", "msi"));
    let cancel = Cancel::new();
    let (_, update, mut net) = staged(tmp.path(), signer.keys(), INSTALLER, &signature);
    net.cancel_on_read = Some(cancel.clone());
    let updater = Updater::with_parts(
        product("0.1.0", signer.keys()),
        InstallKind::Msi,
        tmp.path(),
        Policy::default(),
        Box::new(net),
    );

    let err = updater.download(&update, &cancel, &mut |_| {}).unwrap_err();

    assert!(matches!(err, UpdateError::Cancelled), "{err}");
    let left: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n != "state.json")
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn download_of_wrong_size_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let signer = Signer::new();
    for body in [&b"short"[..], &b"0123456789abcdef and a tail"[..]] {
        let signature = signer.sign(body, &asset("app-0.2.0", "msi"));
        let (updater, update, _net) = staged(tmp.path(), signer.keys(), body, &signature);
        let err = updater.download(&update, &Cancel::new(), &mut |_| {}).unwrap_err();
        assert!(matches!(err, UpdateError::SizeMismatch { expected: 16, .. }), "{err}");
        assert!(!tmp.path().join(&update.asset_name).exists());
    }
}

/// Скачать и проверить; вернуть ошибку подписи и убедиться, что отвергнутый файл удалён.
fn rejected(keys: &'static [&'static str], installer: &[u8], signature: &str) -> SignatureError {
    let tmp = tempfile::tempdir().unwrap();
    let (updater, update, _net) = staged(tmp.path(), keys, installer, signature);
    let downloaded = updater.download(&update, &Cancel::new(), &mut |_| {}).unwrap();
    let err = updater.verify(downloaded.clone()).unwrap_err();
    assert!(!downloaded.file.exists(), "rejected file must be removed");
    assert!(!downloaded.signature.exists());
    match err {
        UpdateError::Signature(e) => e,
        other => panic!("signature error expected, got {other}"),
    }
}

#[test]
fn replaced_file_is_rejected() {
    let signer = Signer::new();
    let name = &asset("app-0.2.0", "msi");
    // Подпись настоящая, файл другой (тот же размер).
    let signature = signer.sign(INSTALLER, name);
    assert_eq!(
        rejected(signer.keys(), b"0123456789abcdeX", &signature),
        SignatureError::Mismatch
    );
    // Файл и подпись согласованы, но подписаны чужим ключом.
    let stranger = Signer::new();
    let signature = stranger.sign(INSTALLER, name);
    assert_eq!(
        rejected(signer.keys(), INSTALLER, &signature),
        SignatureError::UnknownKey
    );
    // Вместо подписи — мусор.
    assert_eq!(
        rejected(signer.keys(), INSTALLER, "not a signature"),
        SignatureError::Malformed
    );
}

/// Откат подменой: под видом 0.2.0 отдают старый установщик 0.1.0 с его настоящей подписью.
#[test]
fn old_signed_installer_is_not_accepted_as_new() {
    let signer = Signer::new();
    let signature = signer.sign(INSTALLER, &asset("app-0.1.0", "msi"));
    assert_eq!(
        rejected(signer.keys(), INSTALLER, &signature),
        SignatureError::WrongFile
    );
    // Как и подписанный файл другого продукта.
    let signature = signer.sign(INSTALLER, &asset("other-0.2.0", "msi"));
    assert_eq!(
        rejected(signer.keys(), INSTALLER, &signature),
        SignatureError::WrongFile
    );
}

#[test]
fn second_embedded_key_is_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let (old, new) = (Signer::new(), Signer::new());
    let keys: &'static [&'static str] = Box::leak(vec![old.keys()[0], new.keys()[0]].into_boxed_slice());
    let signature = new.sign(INSTALLER, &asset("app-0.2.0", "msi"));
    let (updater, update, _net) = staged(tmp.path(), keys, INSTALLER, &signature);
    let downloaded = updater.download(&update, &Cancel::new(), &mut |_| {}).unwrap();
    updater.verify(downloaded).unwrap();
}
