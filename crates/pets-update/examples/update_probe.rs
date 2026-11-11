//! Проба крейта на стенде — то, чего нет в тестах: настоящая сеть и настоящий помощник установки.
//!
//! ```text
//! update_probe check <owner/repo> <prefix> <текущая версия> [beta]   запрос к GitHub Releases, печать итога
//! update_probe prepare <папка>                                        собрать «portable-программу» и подписанный архив
//! <папка>\app\app.exe apply <папка>                            проверить подпись, запустить помощника, выйти
//! ```
//!
//! После `apply` помощник заменяет `app\version.txt` и запускает `app.exe restarted <папка>`, тот пишет
//! `<папка>\restarted.txt`: версия после установки и итог из `take_result`. Ключ подписи создаётся на лету в
//! `prepare` и живёт только в этой папке.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use pets_update::{
    Channel, CheckReason, Downloaded, InstallKind, InstallPlan, Policy, Product, Settings, Update, Updater,
    UreqTransport, Version,
};

const NEW: &str = "9.9.9";

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Продукт пробы; ключ — из `key.txt` в папке на уровень выше exe (там же лежит и копия-помощник).
fn local_product() -> Product {
    let exe = std::env::current_exe().unwrap();
    let key = exe
        .parent()
        .and_then(Path::parent)
        .and_then(|root| fs::read_to_string(root.join("key.txt")).ok());
    Product {
        name: "PetsUpdateProbe",
        repo: "none/none",
        asset_prefix: "app",
        current: Version::new(1, 0, 0),
        public_keys: Box::leak(key.into_iter().map(leak).collect::<Vec<_>>().into_boxed_slice()),
    }
}

fn main() {
    let product = local_product();
    // Первая строка `main` продукта.
    if let Some(code) = pets_update::run_helper(&product) {
        std::process::exit(code);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or_default();
    match arg(0) {
        "check" => check(arg(1), arg(2), arg(3), arg(4) == "beta"),
        "prepare" => prepare(Path::new(arg(1))),
        "apply" => apply(product, Path::new(arg(1))),
        "restarted" => restarted(&product, Path::new(arg(1))),
        _ => eprintln!("usage: update_probe check <owner/repo> <prefix> <version> [beta] | prepare <dir>"),
    }
}

fn check(repo: &str, prefix: &str, version: &str, beta: bool) {
    let product = Product {
        name: "PetsUpdateProbe",
        repo: leak(repo.to_owned()),
        asset_prefix: leak(prefix.to_owned()),
        current: Version::parse(version).unwrap(),
        public_keys: &[],
    };
    let dir = tempfile::tempdir().unwrap();
    let updater = Updater::with_parts(
        product,
        InstallKind::Msi,
        dir.path(),
        Policy::default(),
        Box::new(UreqTransport::new()),
    );
    let settings = Settings {
        enabled: true,
        channel: if beta { Channel::Beta } else { Channel::Stable },
    };
    println!("{:#?}", updater.check(&settings, CheckReason::Manual));
}

fn asset() -> String {
    format!("app-{NEW}-{}-{}.zip", pets_update::OS, pets_update::ARCH)
}

fn prepare(root: &Path) {
    let (app, updates) = (root.join("app"), root.join("updates"));
    fs::create_dir_all(&app).unwrap();
    fs::create_dir_all(&updates).unwrap();
    fs::copy(std::env::current_exe().unwrap(), app.join("app.exe")).unwrap();
    fs::write(app.join("version.txt"), "old").unwrap();
    fs::write(app.join("settings.txt"), "user settings").unwrap();

    let archive = updates.join(asset());
    let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
    for (name, body) in [("version.txt", "new"), ("settings.txt", "defaults")] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();

    let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let comment = format!("timestamp:0\tfile:{}\thashed", asset());
    let signature = minisign::sign(None, &pair.sk, File::open(&archive).unwrap(), Some(&comment), None).unwrap();
    fs::write(updates.join(format!("{}.minisig", asset())), signature.into_string()).unwrap();
    fs::write(root.join("key.txt"), pair.pk.to_base64()).unwrap();
    println!(
        "prepared; run: {} apply {}",
        app.join("app.exe").display(),
        root.display()
    );
}

fn apply(product: Product, root: &Path) {
    let updates = root.join("updates");
    let updater = Updater::with_parts(
        product,
        InstallKind::Portable,
        &updates,
        Policy::default(),
        Box::new(UreqTransport::new()),
    );
    let downloaded = Downloaded {
        update: Update {
            version: Version::parse(NEW).unwrap(),
            kind: InstallKind::Portable,
            page_url: String::new(),
            asset_name: asset(),
            asset_url: String::new(),
            size: 0,
            signature_url: String::new(),
        },
        file: updates.join(asset()),
        signature: updates.join(format!("{}.minisig", asset())),
    };
    let verified = updater.verify(downloaded).unwrap();
    let mut plan = InstallPlan::restart_current().unwrap();
    plan.restart_args = vec!["restarted".to_owned(), root.display().to_string()];
    plan.keep = vec!["settings.txt".to_owned()];
    updater.install(verified, plan).unwrap();
    // Продукт здесь закрывается.
}

fn restarted(product: &Product, root: &Path) {
    let app: PathBuf = root.join("app");
    let report = format!(
        "version={} settings={} result={:?}\n",
        fs::read_to_string(app.join("version.txt")).unwrap_or_default(),
        fs::read_to_string(app.join("settings.txt")).unwrap_or_default(),
        pets_update::take_result(product, &root.join("updates")),
    );
    fs::write(root.join("restarted.txt"), report).unwrap();
}
