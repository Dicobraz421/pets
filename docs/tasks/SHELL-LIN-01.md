# SHELL-LIN-01: pets-shell на Linux — тот же API, что на macOS (паритет Postman)

lane: shell
status: done
deps: []
class: B                    # Cargo.lock (zbus 6), docs/decisions.md
files: [crates/pets-shell/Cargo.toml, crates/pets-shell/src/lib.rs, crates/pets-shell/src/clipboard.rs, crates/pets-shell/src/sys/, Cargo.lock, docs/decisions.md]
read: [Postman docs/decisions/2026-11-12-linux.md, crates/pets-shell/src/sys/macos.rs]
checks: [clippy linux/macos/windows -p pets-shell, nextest -p pets-shell]
manual: yes                 # шаги — ниже (файлы docs/manual-tests — вне полосы)

> Postman `docs/decisions/2026-11-12-linux.md`: Linux tier 2, стандарты freedesktop, без Flatpak.

## Сделать
- Единственный экземпляр — замок и unix-сокет `Pets.<app>.{lock,sock}` в `$XDG_RUNTIME_DIR` (код macOS → `sys/unix.rs`).
- `credentials` — Secret Service (D-Bus, `zbus`); нет службы — `ShellError::Os` с `NotFound`, на диск не пишем.
- `url_scheme` — `.desktop` этого исполняемого файла + `xdg-settings`/`xdg-mime` (`x-scheme-handler/mailto`).
- `login_item` — `~/.config/autostart/<id>.desktop`.
- `memory::phys_footprint` — `/proc/self/smaps_rollup`.
- `console::stderr_to_file` — общий с macOS (`sys/console_unix.rs`).
- `clipboard::read_html`/`has_html` — `text/html` через `wl-paste`/`xclip`; запись HTML — нет.

## Приёмка
- `cargo nextest run -p pets-shell` на Linux: экземпляр, разбор `.desktop`/`Exec`, автозапуск, память, HTML UTF-16.
- `credentials_round_trip` (ignored) — зелёный на gnome-keyring (`dbus-run-session`, коллекция `session` как default).

## Ручная проверка (GNOME/KDE, установленный пакет Postman с `.desktop`)
1. Запустить Postman дважды — второй выходит, первый получает аргументы (`postman mailto:a@b`).
2. Сохранить пароль учётной записи → «Пароли и ключи» (Seahorse)/KWallet: запись `service=<имя>`; перезапуск — пароль читается.
   Без службы (`killall gnome-keyring-daemon`, без активации) — понятная ошибка, файла с паролем нет.
3. «Сделать почтовой программой по умолчанию» → `xdg-mime query default x-scheme-handler/mailto` = `postman.desktop`.
4. «Запускать при входе» → `~/.config/autostart/postman.desktop`; выключить в «Автозапуске» среды → состояние «требует
   разрешения»; выход/вход — Postman стартует.
5. Скопировать таблицу из Firefox и из Chromium (X11 и Wayland) → «Вставить с форматированием» доступно, разметка вставляется.
