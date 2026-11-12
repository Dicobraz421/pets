//! Установка проверенного обновления.
//!
//! Программа не может заменить собственные файлы, пока работает. Поэтому [`Updater::install`] копирует её exe в
//! рабочую папку обновления и запускает копию как помощника; продукт сразу после этого закрывается. Помощник
//! ([`run_helper`], зовётся первой строкой `main` продукта):
//!
//! 1. ждёт выхода программы (конец потока stdin, который держала программа);
//! 2. заново проверяет подпись и держит файл закрытым от записи до конца установки;
//! 3. MSI — `msiexec /i <файл> /passive /norestart`; portable — распаковка и замена файлов рядом с exe с откатом;
//!    бандл macOS — распаковка рядом и замена `.app` целиком с откатом;
//! 4. пишет итог (`result.json`), убирает скачанное и запускает программу снова.
//!
//! При следующем старте продукт читает итог через [`take_result`].

use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::io_err;
use crate::updater::Verified;
use crate::{InstallKind, Product, UpdateError, Updater, app_bundle, verify};

/// Первый аргумент командной строки помощника; второй — путь к файлу задания.
pub const HELPER_FLAG: &str = "--pets-update-apply";

const JOB_FILE: &str = "job.json";
const RESULT_FILE: &str = "result.json";
const STAGING_DIR: &str = ".pets-update-new";
const BACKUP_DIR: &str = ".pets-update-old";

/// Сколько раз повторять переименование занятого файла и с какой паузой: процесс программы уже вышел, но ОС
/// может ещё доли секунды держать его exe.
const RENAME_ATTEMPTS: u32 = 40;
const RENAME_PAUSE: Duration = Duration::from_millis(250);

/// Что делать помощнику после установки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// Exe программы: его папка — место установки portable, его же запускают после установки.
    pub app_exe: PathBuf,
    /// Аргументы перезапуска.
    pub restart_args: Vec<String>,
    /// Portable: файлы (пути от папки программы, через `/`), которые не перезаписываются, если уже есть, —
    /// файл-флаг и настройки, лежащие рядом с exe.
    pub keep: Vec<String>,
}

impl InstallPlan {
    /// Перезапустить текущий exe без аргументов.
    pub fn restart_current() -> Result<InstallPlan, UpdateError> {
        Ok(InstallPlan {
            app_exe: std::env::current_exe().map_err(io_err("find own exe"))?,
            restart_args: Vec::new(),
            keep: Vec::new(),
        })
    }
}

/// Итог последней установки; продукт показывает его после перезапуска.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateResult {
    /// Версия, которую ставили.
    pub version: String,
    /// Установлена ли.
    pub ok: bool,
    /// Текст ошибки для журнала и подробностей уведомления.
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Job {
    /// Имя артефакта: к нему привязана подпись.
    name: String,
    version: String,
    kind: InstallKind,
    file: PathBuf,
    signature: PathBuf,
    /// Рабочая папка обновления.
    dir: PathBuf,
    app_exe: PathBuf,
    restart_args: Vec<String>,
    keep: Vec<String>,
}

impl Updater {
    /// Запустить установку проверенного файла. После `Ok(())` продукт должен сразу завершиться: помощник ждёт
    /// его выхода, ставит обновление и запускает программу снова. Ничего не ставится, пока программа работает.
    pub fn install(&self, verified: Verified, plan: InstallPlan) -> Result<(), UpdateError> {
        let downloaded = &verified.downloaded;
        // Проверка до закрытия программы: в папку без права записи обновление не поставить. Бандл заменяется
        // переименованием — нужна запись в папку, где он лежит (`/Applications`).
        let writable = match downloaded.update.kind {
            InstallKind::Msi => None,
            InstallKind::Portable => Some(
                plan.app_exe
                    .parent()
                    .ok_or(UpdateError::Invalid("app exe has no folder"))?
                    .to_path_buf(),
            ),
            InstallKind::AppBundle => Some(
                app_bundle(&plan.app_exe)
                    .and_then(|b| b.parent().map(Path::to_path_buf))
                    .ok_or(UpdateError::Invalid("program is not in an .app bundle"))?,
            ),
        };
        if let Some(dir) = writable {
            let probe = dir.join(".pets-update-probe");
            File::create(&probe).map_err(io_err("write to program folder"))?;
            let _ = fs::remove_file(&probe);
        }
        let job = Job {
            name: downloaded.update.asset_name.clone(),
            version: downloaded.update.version.to_string(),
            kind: downloaded.update.kind,
            file: downloaded.file.clone(),
            signature: downloaded.signature.clone(),
            dir: self.dir.clone(),
            app_exe: plan.app_exe,
            restart_args: plan.restart_args,
            keep: plan.keep,
        };
        let job_path = self.dir.join(JOB_FILE);
        fs::write(&job_path, serde_json::to_vec(&job)?).map_err(io_err("write update job"))?;

        let helper = self.dir.join(helper_name(&self.product));
        let own = std::env::current_exe().map_err(io_err("find own exe"))?;
        fs::copy(&own, &helper).map_err(io_err("copy update helper"))?;

        let mut command = Command::new(&helper);
        command
            .arg(HELPER_FLAG)
            .arg(&job_path)
            .current_dir(&self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(io_err("start update helper"))?;
        // Пишущий конец stdin помощника живёт до выхода программы: его закрытие — сигнал «программа вышла».
        if let Some(stdin) = child.stdin.take() {
            std::mem::forget(stdin);
        }
        tracing::info!(version = %job.version, "update: helper started, program must exit now");
        Ok(())
    }
}

/// Имя копии exe, работающей помощником.
fn helper_name(product: &Product) -> String {
    format!("{}-update-helper{}", product.asset_prefix, std::env::consts::EXE_SUFFIX)
}

/// Режим помощника. Продукт зовёт первой строкой `main`: `Some(код)` — это был запуск помощника, нужно
/// сразу выйти с этим кодом, не открывая окно; `None` — обычный запуск.
pub fn run_helper(product: &Product) -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != HELPER_FLAG {
        return None;
    }
    let job_path = PathBuf::from(args.next()?);
    let job: Job = match fs::read(&job_path).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        Some(job) => job,
        None => {
            tracing::error!(job = %job_path.display(), "update: job file is not readable");
            return Some(2);
        }
    };
    // Программа держит пишущий конец нашего stdin; конец потока — она вышла.
    let _ = std::io::stdin().read_to_end(&mut Vec::new());

    let outcome = apply(product.public_keys, &job, &mut run_msiexec);
    if let Err(e) = &outcome {
        tracing::error!(error = %e, version = %job.version, "update: install failed");
    }
    let result = UpdateResult {
        version: job.version.clone(),
        ok: outcome.is_ok(),
        error: outcome.as_ref().err().map(ToString::to_string),
    };
    if let Ok(bytes) = serde_json::to_vec(&result) {
        let _ = fs::write(job.dir.join(RESULT_FILE), bytes);
    }
    let _ = fs::remove_file(&job.file);
    let _ = fs::remove_file(&job.signature);
    let _ = fs::remove_file(&job_path);
    // Программа запускается и после неудачи: старая версия на месте, итог она покажет сама.
    if let Err(e) = restart(&job) {
        tracing::error!(error = %e, "update: program is not restarted");
    }
    Some(i32::from(outcome.is_err()))
}

/// Запустить программу снова. Бандл macOS — через `open`: Launch Services узнаёт новую версию бандла (значок, Dock).
fn restart(job: &Job) -> std::io::Result<std::process::Child> {
    if job.kind == InstallKind::AppBundle
        && cfg!(target_os = "macos")
        && let Some(bundle) = app_bundle(&job.app_exe)
    {
        let mut open = Command::new("/usr/bin/open");
        open.arg(bundle);
        if !job.restart_args.is_empty() {
            open.arg("--args").args(&job.restart_args);
        }
        return open.spawn();
    }
    Command::new(&job.app_exe).args(&job.restart_args).spawn()
}

/// Прочитать и убрать итог последней установки и копию exe-помощника. Продукт зовёт при старте.
pub fn take_result(product: &Product, dir: &Path) -> Option<UpdateResult> {
    let _ = fs::remove_file(dir.join(helper_name(product)));
    let path = dir.join(RESULT_FILE);
    let bytes = fs::read(&path).ok()?;
    let _ = fs::remove_file(&path);
    serde_json::from_slice(&bytes).ok()
}

/// Проверить подпись и поставить. Файл закрыт от записи от проверки до конца установки.
fn apply(keys: &[&str], job: &Job, msi: &mut dyn FnMut(&Path) -> Result<i32, UpdateError>) -> Result<(), UpdateError> {
    let signature = fs::read_to_string(&job.signature).map_err(io_err("read signature"))?;
    let lock = verify::verify_file(keys, &job.name, &job.file, &signature)?;
    match job.kind {
        InstallKind::Msi => match msi(&job.file)? {
            // 3010 и 1641 — установлено, нужна или начата перезагрузка.
            0 | 3010 | 1641 => Ok(()),
            code => Err(UpdateError::InstallerFailed(code)),
        },
        InstallKind::Portable => {
            let app_dir = job
                .app_exe
                .parent()
                .ok_or(UpdateError::Invalid("app exe has no folder"))?;
            apply_portable(&lock, app_dir, &job.keep, RENAME_ATTEMPTS)
        }
        InstallKind::AppBundle => {
            let bundle = app_bundle(&job.app_exe).ok_or(UpdateError::Invalid("program is not in an .app bundle"))?;
            let exe = job
                .app_exe
                .file_name()
                .ok_or(UpdateError::Invalid("app exe has no name"))?;
            apply_bundle(&lock, &bundle, Path::new(exe), RENAME_ATTEMPTS)
        }
    }
}

/// `msiexec /i <файл> /passive /norestart`: полоса хода без вопросов; запрос UAC покажет сам установщик.
fn run_msiexec(file: &Path) -> Result<i32, UpdateError> {
    if !cfg!(windows) {
        return Err(UpdateError::Invalid("msi is installed only on Windows"));
    }
    // Полный путь: не искать `msiexec` в текущей папке и PATH.
    let root = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let status = Command::new(root.join("System32").join("msiexec.exe"))
        .arg("/i")
        .arg(file)
        .args(["/passive", "/norestart"])
        .status()
        .map_err(io_err("run msiexec"))?;
    Ok(status.code().unwrap_or(-1))
}

/// Распаковать архив в папку программы: сначала всё во временную подпапку, затем замена по одному файлу со
/// старым в запасе; при ошибке — откат к прежним файлам. Файлы, которых нет в архиве, не трогаются.
fn apply_portable(archive: &File, app_dir: &Path, keep: &[String], attempts: u32) -> Result<(), UpdateError> {
    let staging = app_dir.join(STAGING_DIR);
    let backup = app_dir.join(BACKUP_DIR);
    for dir in [&staging, &backup] {
        if dir.exists() {
            fs::remove_dir_all(dir).map_err(io_err("clean previous update"))?;
        }
    }
    let files = extract(archive, &staging).inspect_err(|_| {
        let _ = fs::remove_dir_all(&staging);
    })?;

    let mut moved: Vec<&PathBuf> = Vec::new();
    let mut placed: Vec<&PathBuf> = Vec::new();
    let swapped = files.iter().try_for_each(|rel| {
        let target = app_dir.join(rel);
        let kept = keep.iter().any(|k| Path::new(k) == rel.as_path());
        if kept && target.exists() {
            return Ok(());
        }
        if target.exists() {
            let old = backup.join(rel);
            make_parent(&old)?;
            rename_retry(&target, &old, attempts).map_err(io_err("replace program file"))?;
            moved.push(rel);
        }
        make_parent(&target)?;
        rename_retry(&staging.join(rel), &target, attempts).map_err(io_err("place program file"))?;
        placed.push(rel);
        Ok::<(), UpdateError>(())
    });
    if swapped.is_err() {
        tracing::warn!("update: portable replace failed, rolling back");
        for rel in placed.iter().rev() {
            let _ = fs::remove_file(app_dir.join(rel));
        }
        for rel in moved.iter().rev() {
            let _ = fs::rename(backup.join(rel), app_dir.join(rel));
        }
    }
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_dir_all(&backup);
    swapped
}

/// Заменить бандл `.app` целиком новым из архива. Архив распаковывается рядом с бандлом (тот же том — замена
/// переименованием); в нём ровно один `*.app` верхнего уровня с `Contents/MacOS/<exe>`, прочее (`README.txt`)
/// не ставится. Старый бандл уходит в запас и возвращается, если новый не встал; после успеха запас удаляется.
fn apply_bundle(archive: &File, bundle: &Path, exe: &Path, attempts: u32) -> Result<(), UpdateError> {
    let parent = bundle
        .parent()
        .ok_or(UpdateError::Invalid("app bundle has no folder"))?;
    let name = bundle
        .file_name()
        .ok_or(UpdateError::Invalid("app bundle has no name"))?
        .to_string_lossy();
    let staging = parent.join(format!("{STAGING_DIR}-{name}"));
    let backup = parent.join(format!("{BACKUP_DIR}-{name}"));
    // Прошлая установка оборвалась между переименованиями: бандла нет, есть запас — вернуть его.
    if !bundle.exists() && backup.exists() {
        fs::rename(&backup, bundle).map_err(io_err("restore previous app bundle"))?;
    }
    for dir in [&staging, &backup] {
        if dir.exists() {
            fs::remove_dir_all(dir).map_err(io_err("clean previous update"))?;
        }
    }
    let placed = extract(archive, &staging).and_then(|_| {
        let new = new_bundle(&staging, exe)?;
        rename_retry(bundle, &backup, attempts).map_err(io_err("move old app bundle"))?;
        if let Err(e) = rename_retry(&new, bundle, attempts) {
            tracing::warn!("update: app bundle replace failed, rolling back");
            let _ = fs::rename(&backup, bundle);
            return Err(io_err("place new app bundle")(e));
        }
        Ok(())
    });
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_dir_all(&backup);
    placed
}

/// Единственный `*.app` верхнего уровня распакованного архива, с программой `Contents/MacOS/<exe>`.
fn new_bundle(staging: &Path, exe: &Path) -> Result<PathBuf, UpdateError> {
    let mut apps = fs::read_dir(staging)
        .map_err(io_err("read unpacked archive"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")));
    let (Some(app), None) = (apps.next(), apps.next()) else {
        return Err(UpdateError::Archive(
            "archive must hold exactly one .app bundle".to_owned(),
        ));
    };
    if !app.join("Contents").join("MacOS").join(exe).is_file() {
        return Err(UpdateError::Archive(format!(
            "no Contents/MacOS/{} in the .app bundle",
            exe.display()
        )));
    }
    Ok(app)
}

/// Распаковать архив в `staging`; вернуть относительные пути файлов и ссылок. Путь, выходящий из папки, — ошибка.
/// Unix: права файлов (бит запуска) — из архива; символьная ссылка — только внутрь распаковки. `__MACOSX/` (ресурсы
/// и расширенные атрибуты, их кладёт `ditto --sequesterRsrc`) пропускается.
fn extract(archive: &File, staging: &Path) -> Result<Vec<PathBuf>, UpdateError> {
    let bad = |e: zip::result::ZipError| UpdateError::Archive(e.to_string());
    let mut zip = zip::ZipArchive::new(archive).map_err(bad)?;
    let mut files = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(bad)?;
        let rel = entry
            .enclosed_name()
            .filter(|p| p.components().all(|c| matches!(c, Component::Normal(_))))
            .ok_or_else(|| UpdateError::Archive(format!("unsafe path in archive, entry {i}")))?;
        if rel.starts_with("__MACOSX") {
            continue;
        }
        let out = staging.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(io_err("unpack archive"))?;
            continue;
        }
        make_parent(&out)?;
        if entry.is_symlink() {
            let mut target = String::new();
            entry.read_to_string(&mut target).map_err(io_err("unpack archive"))?;
            symlink(&rel, Path::new(&target), &out)?;
            files.push(rel);
            continue;
        }
        let mut file = File::create(&out).map_err(io_err("unpack archive"))?;
        std::io::copy(&mut entry, &mut file).map_err(io_err("unpack archive"))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(mode & 0o777))
                .map_err(io_err("unpack archive"))?;
        }
        files.push(rel);
    }
    Ok(files)
}

/// Ссылка `rel` → `target` (относительно папки ссылки) остаётся внутри распаковки.
fn link_inside(rel: &Path, target: &Path) -> bool {
    let mut depth = rel.components().count().saturating_sub(1);
    for c in target.components() {
        match c {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    true
}

/// Символьная ссылка из архива (фреймворки в бандле `.app`). Вне Unix ссылок в архивах обновления нет.
fn symlink(rel: &Path, target: &Path, out: &Path) -> Result<(), UpdateError> {
    if !link_inside(rel, target) {
        return Err(UpdateError::Archive(format!(
            "link {} points outside the archive",
            rel.display()
        )));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, out).map_err(io_err("unpack archive"))
    }
    #[cfg(not(unix))]
    {
        let _ = out;
        Err(UpdateError::Archive(format!("link {} in archive", rel.display())))
    }
}

fn make_parent(path: &Path) -> Result<(), UpdateError> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent).map_err(io_err("create folder")),
        None => Ok(()),
    }
}

fn rename_retry(from: &Path, to: &Path, attempts: u32) -> std::io::Result<()> {
    let mut left = attempts.max(1);
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if left <= 1 => return Err(e),
            Err(_) => {
                left -= 1;
                std::thread::sleep(RENAME_PAUSE);
            }
        }
    }
}

#[cfg(test)]
mod tests;
