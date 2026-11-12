//! Ручная проверка журнала: `cargo run -p pets-shell --example probe -- error|warn|info|panic|fatal|second [арг…]`.
//! Записи — в stderr и в журнале Windows «Приложение», источник `PetsProbe` (просмотр событий).
//! `second` — единственный экземпляр: первый запуск ждёт аргументы, второй передаёт свои и выходит.

#[expect(clippy::panic, reason = "паника — то, что проверяет пример")]
fn main() {
    pets_shell::init_logging("PetsProbe", "PETS_PROBE_LOG");
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("error") => tracing::error!(probe = "error", "проба: ошибка в журнал Windows"),
        Some("warn") => tracing::warn!(probe = "warn", "проба: предупреждение в журнал Windows"),
        Some("info") => tracing::info!(probe = "info", "проба: сведения в журнал Windows"),
        Some("panic") => std::panic::panic_any("проба: паника".to_owned()),
        Some("fatal") => pets_shell::fatal("проба: запуск невозможен", "нет профиля"),
        Some("second") => match pets_shell::single_instance("PetsProbe", &args[1..]) {
            Ok(pets_shell::Instance::First(listener)) => {
                tracing::info!("первый экземпляр; запустите `probe -- second …` ещё раз, Ctrl+C — выход");
                listener.listen(|args| tracing::info!("аргументы второго: {args:?}"));
                std::thread::park();
            }
            Ok(pets_shell::Instance::Second) => tracing::info!("второй экземпляр: аргументы переданы"),
            Err(e) => tracing::warn!("единственный экземпляр не проверен: {e}"),
        },
        _ => eprintln!("probe -- error|warn|info|panic|fatal|second [арг…]"),
    }
}
