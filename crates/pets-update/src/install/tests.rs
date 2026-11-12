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

// ── Бандл macOS (.app из .zip) ──────────────────────────────────────────────────────────────────────────────────

/// Старый бандл `App.app` в папке `Applications`: программа, ресурс, которого в новой версии нет.
fn old_bundle(root: &Path) -> PathBuf {
    let bundle = root.join("Applications").join("App.app");
    fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
    fs::write(bundle.join("Contents/MacOS/app"), "old app").unwrap();
    fs::write(bundle.join("Contents/Resources/gone.txt"), "old only").unwrap();
    bundle
}

#[test]
fn app_bundle_is_found_from_exe() {
    let exe = Path::new("/Applications/Postman.app/Contents/MacOS/postman");
    assert_eq!(app_bundle(exe), Some(PathBuf::from("/Applications/Postman.app")));
    assert_eq!(app_bundle(Path::new("/Users/me/target/release/postman")), None);
    assert_eq!(
        app_bundle(Path::new("/Applications/Postman/Contents/MacOS/postman")),
        None
    );
}

/// Бандл заменяется целиком: файлов старой версии не остаётся, `README.txt` и `__MACOSX/` архива не ставятся.
#[test]
fn bundle_is_replaced_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let bundle = old_bundle(tmp.path());
    let archive = zip_file(
        tmp.path(),
        &[
            ("App.app/Contents/MacOS/app", "new app"),
            ("App.app/Contents/Info.plist", "plist"),
            ("README.txt", "readme"),
            ("__MACOSX/App.app/._Info.plist", "rsrc"),
        ],
    );

    apply_bundle(&File::open(&archive).unwrap(), &bundle, Path::new("app"), 1).unwrap();

    assert_eq!(read(&bundle.join("Contents/MacOS/app")), "new app");
    assert_eq!(read(&bundle.join("Contents/Info.plist")), "plist");
    assert!(!bundle.join("Contents/Resources/gone.txt").exists());
    let left: Vec<_> = fs::read_dir(bundle.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["App.app"]);
}

/// Архив без бандла или без программы в нём — ошибка, старый бандл на месте.
#[test]
fn bundle_rejects_archive_without_app() {
    let tmp = tempfile::tempdir().unwrap();
    let bundle = old_bundle(tmp.path());
    for entries in [
        &[("README.txt", "readme")][..],
        &[("App.app/Contents/MacOS/other", "new")][..],
        &[("A.app/Contents/MacOS/app", "a"), ("B.app/Contents/MacOS/app", "b")][..],
    ] {
        let archive = zip_file(tmp.path(), entries);
        let err = apply_bundle(&File::open(&archive).unwrap(), &bundle, Path::new("app"), 1).unwrap_err();
        assert!(matches!(err, UpdateError::Archive(_)), "{err}");
        assert_eq!(read(&bundle.join("Contents/MacOS/app")), "old app");
        assert_eq!(read(&bundle.join("Contents/Resources/gone.txt")), "old only");
        let left: Vec<_> = fs::read_dir(bundle.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["App.app"]);
    }
}

/// Прошлая установка оборвалась после ухода старого бандла в запас: он возвращается и заменяется как обычно.
#[test]
fn bundle_restores_backup_of_interrupted_update() {
    let tmp = tempfile::tempdir().unwrap();
    let bundle = old_bundle(tmp.path());
    let backup = bundle.with_file_name(format!("{BACKUP_DIR}-App.app"));
    fs::rename(&bundle, &backup).unwrap();
    let broken = zip_file(tmp.path(), &[("README.txt", "readme")]);

    let err = apply_bundle(&File::open(&broken).unwrap(), &bundle, Path::new("app"), 1).unwrap_err();

    assert!(matches!(err, UpdateError::Archive(_)), "{err}");
    assert_eq!(read(&bundle.join("Contents/MacOS/app")), "old app");
    assert!(!backup.exists());
}

/// Занятый файл в старом бандле (Windows не даёт переименовать папку): новая версия не ставится, старая цела.
#[cfg(windows)]
#[test]
fn bundle_stays_when_busy() {
    use std::os::windows::fs::OpenOptionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let bundle = old_bundle(tmp.path());
    let archive = zip_file(tmp.path(), &[("App.app/Contents/MacOS/app", "new app")]);
    let busy = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(bundle.join("Contents/MacOS/app"))
        .unwrap();

    let err = apply_bundle(&File::open(&archive).unwrap(), &bundle, Path::new("app"), 1).unwrap_err();
    drop(busy);

    assert!(matches!(err, UpdateError::Io { .. }), "{err}");
    assert_eq!(read(&bundle.join("Contents/MacOS/app")), "old app");
    let left: Vec<_> = fs::read_dir(bundle.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["App.app"]);
}

/// Unix: бит запуска и ссылки внутри бандла сохраняются; ссылка наружу — ошибка.
#[cfg(unix)]
#[test]
fn bundle_keeps_modes_and_links() {
    use std::os::unix::fs::PermissionsExt;
    use zip::write::SimpleFileOptions;

    let tmp = tempfile::tempdir().unwrap();
    let bundle = old_bundle(tmp.path());
    let archive = |links: &[(&str, &str)]| {
        let path = tmp.path().join("update.zip");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        zip.start_file(
            "App.app/Contents/MacOS/app",
            SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
        zip.write_all(b"new app").unwrap();
        zip.start_file(
            "App.app/Contents/Info.plist",
            SimpleFileOptions::default().unix_permissions(0o644),
        )
        .unwrap();
        zip.write_all(b"plist").unwrap();
        for (name, target) in links {
            zip.add_symlink(*name, *target, SimpleFileOptions::default()).unwrap();
        }
        zip.finish().unwrap();
        path
    };

    let good = archive(&[("App.app/Contents/Current", "MacOS")]);
    apply_bundle(&File::open(&good).unwrap(), &bundle, Path::new("app"), 1).unwrap();
    let mode = fs::metadata(bundle.join("Contents/MacOS/app"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
    let plist = fs::metadata(bundle.join("Contents/Info.plist"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(plist & 0o111, 0);
    assert_eq!(
        fs::read_link(bundle.join("Contents/Current")).unwrap(),
        Path::new("MacOS")
    );

    let evil = archive(&[("App.app/Contents/Out", "../../../outside")]);
    let err = apply_bundle(&File::open(&evil).unwrap(), &bundle, Path::new("app"), 1).unwrap_err();
    assert!(matches!(err, UpdateError::Archive(_)), "{err}");
    assert_eq!(read(&bundle.join("Contents/MacOS/app")), "new app");
}

#[test]
fn links_stay_inside_archive() {
    assert!(link_inside(Path::new("A.app/Contents/Current"), Path::new("MacOS")));
    assert!(link_inside(
        Path::new("A.app/Contents/Frameworks/X/Current"),
        Path::new("../../MacOS")
    ));
    assert!(!link_inside(
        Path::new("A.app/Contents/Out"),
        Path::new("../../../outside")
    ));
    assert!(!link_inside(Path::new("link"), Path::new("/etc/passwd")));
}
