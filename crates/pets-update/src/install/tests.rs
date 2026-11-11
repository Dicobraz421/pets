//! Тесты установки: замена файлов portable, откат, опасные пути, повторная проверка подписи помощником.

use std::io::Write;

use super::*;
use crate::SignatureError;

fn zip_file(dir: &Path, entries: &[(&str, &str)]) -> PathBuf {
    let path = dir.join("update.zip");
    let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
    for (name, body) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    path
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn portable_replaces_files_and_keeps_listed() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app");
    fs::create_dir_all(app.join("res")).unwrap();
    fs::write(app.join("app.exe"), "old exe").unwrap();
    fs::write(app.join("res/data.bin"), "old data").unwrap();
    fs::write(app.join("app.portable"), "user flag").unwrap();
    fs::write(app.join("notes.txt"), "user file").unwrap();
    let archive = zip_file(
        tmp.path(),
        &[
            ("app.exe", "new exe"),
            ("res/data.bin", "new data"),
            ("res/extra.bin", "added"),
            ("app.portable", ""),
        ],
    );

    apply_portable(&File::open(&archive).unwrap(), &app, &["app.portable".to_owned()], 1).unwrap();

    assert_eq!(read(&app.join("app.exe")), "new exe");
    assert_eq!(read(&app.join("res/data.bin")), "new data");
    assert_eq!(read(&app.join("res/extra.bin")), "added");
    assert_eq!(read(&app.join("app.portable")), "user flag");
    assert_eq!(read(&app.join("notes.txt")), "user file");
    assert!(!app.join(STAGING_DIR).exists());
    assert!(!app.join(BACKUP_DIR).exists());
}

/// Занятый файл посреди замены: всё уже заменённое возвращается, новая версия не остаётся наполовину.
#[cfg(windows)]
#[test]
fn portable_rolls_back_when_file_is_busy() {
    use std::os::windows::fs::OpenOptionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("a.exe"), "old a").unwrap();
    fs::write(app.join("b.dll"), "old b").unwrap();
    let archive = zip_file(
        tmp.path(),
        &[("a.exe", "new a"), ("b.dll", "new b"), ("c.txt", "new c")],
    );
    let busy = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(app.join("b.dll"))
        .unwrap();

    let err = apply_portable(&File::open(&archive).unwrap(), &app, &[], 1).unwrap_err();
    drop(busy);

    assert!(matches!(err, UpdateError::Io { .. }), "{err}");
    assert_eq!(read(&app.join("a.exe")), "old a");
    assert_eq!(read(&app.join("b.dll")), "old b");
    assert!(!app.join("c.txt").exists());
    assert!(!app.join(STAGING_DIR).exists());
    assert!(!app.join(BACKUP_DIR).exists());
}

#[test]
fn portable_rejects_path_outside_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app");
    fs::create_dir_all(&app).unwrap();
    let archive = zip_file(tmp.path(), &[("ok.txt", "ok"), ("../evil.txt", "evil")]);

    let err = apply_portable(&File::open(&archive).unwrap(), &app, &[], 1).unwrap_err();

    assert!(matches!(err, UpdateError::Archive(_)), "{err}");
    assert!(!tmp.path().join("evil.txt").exists());
    assert!(!app.join("ok.txt").exists());
    assert!(!app.join(STAGING_DIR).exists());
}

struct Signed {
    _tmp: tempfile::TempDir,
    job: Job,
    key: String,
}

/// Подписанный «установщик» во временной папке и задание помощнику.
fn signed_job(kind: InstallKind) -> Signed {
    let tmp = tempfile::tempdir().unwrap();
    let name = format!("app-1.2.0-windows-x64.{}", kind.extension());
    let file = tmp.path().join(&name);
    fs::write(&file, b"installer bytes").unwrap();
    let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let signature = minisign::sign(
        None,
        &pair.sk,
        File::open(&file).unwrap(),
        Some(&format!("timestamp:1\tfile:{name}\thashed")),
        None,
    )
    .unwrap();
    let signature_path = tmp.path().join(format!("{name}.minisig"));
    fs::write(&signature_path, signature.into_string()).unwrap();
    let job = Job {
        name,
        version: "1.2.0".to_owned(),
        kind,
        file,
        signature: signature_path,
        dir: tmp.path().to_owned(),
        app_exe: tmp.path().join("app").join("app.exe"),
        restart_args: Vec::new(),
        keep: Vec::new(),
    };
    Signed {
        key: pair.pk.to_base64(),
        job,
        _tmp: tmp,
    }
}

#[test]
fn helper_runs_installer_for_signed_file() {
    let signed = signed_job(InstallKind::Msi);
    let mut ran = Vec::new();
    apply(&[&signed.key], &signed.job, &mut |file| {
        ran.push(file.to_owned());
        Ok(0)
    })
    .unwrap();
    assert_eq!(ran, std::slice::from_ref(&signed.job.file));

    let err = apply(&[&signed.key], &signed.job, &mut |_| Ok(1603)).unwrap_err();
    assert!(matches!(err, UpdateError::InstallerFailed(1603)), "{err}");
    apply(&[&signed.key], &signed.job, &mut |_| Ok(3010)).unwrap();
}

/// Файл подменили после проверки в программе, до запуска помощника: установщик не запускается.
#[test]
fn helper_does_not_run_replaced_file() {
    let signed = signed_job(InstallKind::Msi);
    fs::write(&signed.job.file, b"installer bytes, but evil").unwrap();
    let mut ran = false;
    let err = apply(&[&signed.key], &signed.job, &mut |_| {
        ran = true;
        Ok(0)
    })
    .unwrap_err();
    assert!(matches!(err, UpdateError::Signature(SignatureError::Mismatch)), "{err}");
    assert!(!ran);
}

/// Пока идёт установка, проверенный файл закрыт от записи и удаления.
#[cfg(windows)]
#[test]
fn helper_locks_file_while_installing() {
    let signed = signed_job(InstallKind::Msi);
    apply(&[&signed.key], &signed.job, &mut |file| {
        assert!(fs::write(file, b"evil").is_err());
        assert!(fs::remove_file(file).is_err());
        Ok(0)
    })
    .unwrap();
}

#[test]
fn result_is_taken_once() {
    let tmp = tempfile::tempdir().unwrap();
    let product = Product {
        name: "App",
        repo: "owner/app",
        asset_prefix: "app",
        current: semver::Version::new(1, 0, 0),
        public_keys: &[],
    };
    assert_eq!(take_result(&product, tmp.path()), None);
    let result = UpdateResult {
        version: "1.2.0".to_owned(),
        ok: false,
        error: Some("installer failed with code 1603".to_owned()),
    };
    fs::write(tmp.path().join(RESULT_FILE), serde_json::to_vec(&result).unwrap()).unwrap();
    fs::write(tmp.path().join(helper_name(&product)), b"exe").unwrap();

    assert_eq!(take_result(&product, tmp.path()), Some(result));
    assert_eq!(take_result(&product, tmp.path()), None);
    assert!(!tmp.path().join(helper_name(&product)).exists());
}
