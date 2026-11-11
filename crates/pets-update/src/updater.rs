//! [`Updater`]: проверка по расписанию и по кнопке, скачивание с прогрессом и отменой, проверка подписи.
//!
//! Все методы блокирующие: продукт вызывает их из фонового исполнителя, не из UI-потока.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::io_err;
use crate::release::{self, SIGNATURE_EXT, Update};
use crate::transport::{Transport, UreqTransport};
use crate::{InstallKind, Policy, Product, Settings, UpdateError, verify};

/// Период проверки по расписанию.
pub const CHECK_PERIOD: Duration = Duration::from_secs(24 * 60 * 60);

/// Пауза после неудачной попытки (нет сети, ошибка сервера), чтобы не стучаться при каждом вызове.
pub const RETRY_PERIOD: Duration = Duration::from_secs(60 * 60);

/// Предел ответа со списком выпусков и файла подписи.
const MAX_LIST: u64 = 4 * 1024 * 1024;
const MAX_SIGNATURE: u64 = 16 * 1024;

const STATE_FILE: &str = "state.json";

/// Кто просит проверку.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckReason {
    /// Фоновая проверка: не чаще раза в сутки и только при включённой настройке.
    Scheduled,
    /// Кнопка «Проверить»: сразу и при выключенной настройке.
    Manual,
}

/// Чем выключено обновление.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledBy {
    /// Групповой политикой: не работает и кнопка.
    Policy,
    /// Настройкой пользователя: кнопка работает.
    Settings,
}

/// Итог проверки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// Выключено — запроса не было.
    Disabled(DisabledBy),
    /// Сутки с прошлой проверки не прошли — запроса не было.
    NotDue,
    /// Новее текущей версии ничего нет.
    UpToDate,
    /// Вышла новая версия.
    Available(Update),
}

/// Токен отмены скачивания; клонируется в UI.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// Новый токен.
    pub fn new() -> Cancel {
        Cancel::default()
    }

    /// Отменить.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Отменено ли.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Ход скачивания.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Получено байт.
    pub received: u64,
    /// Всего байт.
    pub total: u64,
}

/// Скачанный, но ещё не проверенный артефакт.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    /// Что скачано.
    pub update: Update,
    /// Файл артефакта.
    pub file: PathBuf,
    /// Файл подписи `<артефакт>.minisig`.
    pub signature: PathBuf,
}

/// Артефакт с проверенной подписью. Пока значение живо, файл закрыт от записи другими процессами.
/// Единственный вход установки — [`Updater::install`](crate::Updater::install).
#[derive(Debug)]
pub struct Verified {
    pub(crate) downloaded: Downloaded,
    pub(crate) _lock: File,
}

impl Verified {
    /// Что проверено.
    pub fn update(&self) -> &Update {
        &self.downloaded.update
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    /// Последняя попытка проверки, секунды Unix.
    #[serde(default)]
    last_attempt: Option<u64>,
    /// Последняя удачная проверка, секунды Unix.
    #[serde(default)]
    last_success: Option<u64>,
}

/// Обновление одного продукта.
pub struct Updater {
    pub(crate) product: Product,
    pub(crate) kind: InstallKind,
    pub(crate) dir: PathBuf,
    policy: Policy,
    transport: Box<dyn Transport>,
}

impl Updater {
    /// `kind` — как установлена программа ([`InstallKind::detect`]); `dir` — рабочая папка обновления в профиле
    /// (`%LOCALAPPDATA%\<Продукт>\updates`): скачанный файл, время последней проверки, итог установки.
    /// Политика читается из реестра здесь, один раз; сети в конструкторе нет.
    pub fn new(product: Product, kind: InstallKind, dir: impl Into<PathBuf>) -> Updater {
        let policy = Policy::read(product.name);
        Updater::with_parts(product, kind, dir, policy, Box::new(UreqTransport::new()))
    }

    /// То же с заданными политикой и сетью — для тестов продукта.
    pub fn with_parts(
        product: Product,
        kind: InstallKind,
        dir: impl Into<PathBuf>,
        policy: Policy,
        transport: Box<dyn Transport>,
    ) -> Updater {
        Updater {
            product,
            kind,
            dir: dir.into(),
            policy,
            transport,
        }
    }

    /// Политика, прочитанная при создании: UI по ней прячет выключатель и кнопку.
    pub fn policy(&self) -> Policy {
        self.policy
    }

    /// Проверить, нет ли новой версии. Запрос идёт только если обновление не выключено политикой и: по кнопке —
    /// всегда; по расписанию — при включённой настройке и не чаще раза в сутки (после ошибки — раза в час).
    pub fn check(&self, settings: &Settings, reason: CheckReason) -> Result<CheckOutcome, UpdateError> {
        self.check_at(settings, reason, SystemTime::now())
    }

    /// [`check`](Self::check) с заданным «сейчас» — для тестов расписания.
    pub fn check_at(
        &self,
        settings: &Settings,
        reason: CheckReason,
        now: SystemTime,
    ) -> Result<CheckOutcome, UpdateError> {
        if !self.policy.updates_allowed() {
            return Ok(CheckOutcome::Disabled(DisabledBy::Policy));
        }
        let now_secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let mut state = self.load_state();
        if reason == CheckReason::Scheduled {
            if !settings.enabled {
                return Ok(CheckOutcome::Disabled(DisabledBy::Settings));
            }
            if !due(state.last_success, now_secs, CHECK_PERIOD) || !due(state.last_attempt, now_secs, RETRY_PERIOD) {
                return Ok(CheckOutcome::NotDue);
            }
        }
        state.last_attempt = Some(now_secs);
        self.save_state(&state);

        let url = release::releases_url(self.product.repo);
        tracing::debug!(%url, "update: checking");
        let mut json = String::new();
        self.transport
            .get(&url)?
            .body
            .take(MAX_LIST)
            .read_to_string(&mut json)
            .map_err(|e| UpdateError::Network(e.to_string()))?;
        let found = release::select(&json, &self.product, settings.channel, self.kind)?;

        state.last_success = Some(now_secs);
        self.save_state(&state);
        Ok(match found {
            Some(update) => {
                tracing::info!(version = %update.version, "update: new version available");
                CheckOutcome::Available(update)
            }
            None => CheckOutcome::UpToDate,
        })
    }

    /// Скачать артефакт и его подпись в рабочую папку. Вызывается только после согласия пользователя.
    /// `progress` зовётся не чаще ~10 раз в секунду и в конце; отмена — через `cancel`, недокачанное удаляется.
    pub fn download(
        &self,
        update: &Update,
        cancel: &Cancel,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Downloaded, UpdateError> {
        if !self.policy.updates_allowed() {
            return Err(UpdateError::DisabledByPolicy);
        }
        // Имя файла на диске — только вычисленное из продукта и версии, не присланное сервером.
        let name = self.product.asset_name(&update.version, update.kind);
        if name != update.asset_name || !update.asset_url.starts_with("https://") {
            return Err(UpdateError::Invalid("update does not belong to this product"));
        }
        fs::create_dir_all(&self.dir).map_err(io_err("create update folder"))?;
        let file = self.dir.join(&name);
        let signature = self.dir.join(format!("{name}.{SIGNATURE_EXT}"));
        let part = self.dir.join(format!("{name}.part"));

        let mut text = Vec::new();
        self.transport
            .get(&update.signature_url)?
            .body
            .take(MAX_SIGNATURE)
            .read_to_end(&mut text)
            .map_err(|e| UpdateError::Network(e.to_string()))?;
        fs::write(&signature, &text).map_err(io_err("write signature"))?;

        let result = self.fetch(update, &part, cancel, progress);
        if let Err(e) = result {
            let _ = fs::remove_file(&part);
            let _ = fs::remove_file(&signature);
            return Err(e);
        }
        let _ = fs::remove_file(&file);
        fs::rename(&part, &file).map_err(io_err("rename downloaded file"))?;
        tracing::info!(file = %file.display(), "update: downloaded");
        Ok(Downloaded {
            update: update.clone(),
            file,
            signature,
        })
    }

    fn fetch(
        &self,
        update: &Update,
        part: &Path,
        cancel: &Cancel,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<(), UpdateError> {
        let mut response = self.transport.get(&update.asset_url)?;
        let mut out = File::create(part).map_err(io_err("create download file"))?;
        let total = update.size;
        let mut received = 0u64;
        let mut buf = vec![0u8; 64 * 1024];
        let mut shown = Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(UpdateError::Cancelled);
            }
            let n = response
                .body
                .read(&mut buf)
                .map_err(|e| UpdateError::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            received += n as u64;
            if received > total {
                return Err(UpdateError::SizeMismatch {
                    expected: total,
                    got: received,
                });
            }
            out.write_all(&buf[..n]).map_err(io_err("write download file"))?;
            if shown.elapsed() >= Duration::from_millis(100) {
                shown = Instant::now();
                progress(Progress { received, total });
            }
        }
        if received != total {
            return Err(UpdateError::SizeMismatch {
                expected: total,
                got: received,
            });
        }
        out.sync_all().map_err(io_err("write download file"))?;
        progress(Progress { received, total });
        Ok(())
    }

    /// Проверить подпись скачанного файла вшитым ключом. Не прошла — файл и подпись удаляются, ошибка
    /// [`UpdateError::Signature`]; установить такой файл нельзя: [`Verified`] создаётся только здесь.
    pub fn verify(&self, downloaded: Downloaded) -> Result<Verified, UpdateError> {
        let checked = fs::read_to_string(&downloaded.signature)
            .map_err(io_err("read signature"))
            .and_then(|text| {
                verify::verify_file(
                    self.product.public_keys,
                    &downloaded.update.asset_name,
                    &downloaded.file,
                    &text,
                )
            });
        match checked {
            Ok(lock) => Ok(Verified {
                downloaded,
                _lock: lock,
            }),
            Err(e) => {
                tracing::error!(file = %downloaded.file.display(), error = %e, "update: file rejected and removed");
                let _ = fs::remove_file(&downloaded.file);
                let _ = fs::remove_file(&downloaded.signature);
                Err(e)
            }
        }
    }

    fn load_state(&self) -> State {
        fs::read_to_string(self.dir.join(STATE_FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_state(&self, state: &State) {
        let saved = fs::create_dir_all(&self.dir)
            .and_then(|()| fs::write(self.dir.join(STATE_FILE), serde_json::to_vec(state).unwrap_or_default()));
        if let Err(e) = saved {
            tracing::warn!(error = %e, "update: check time is not saved");
        }
    }
}

/// Прошёл ли период с момента `last`. Часы ушли назад (`last` в будущем) — считаем, что прошёл.
fn due(last: Option<u64>, now: u64, period: Duration) -> bool {
    last.is_none_or(|t| t > now || now - t >= period.as_secs())
}
