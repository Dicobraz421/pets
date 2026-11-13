# PAR-03: pets-shell — одно поведение на всех ОС: пароль, системный журнал, журнал в файл, единственный экземпляр

lane: shell
status: done
deps: [SHELL-LIN-01]
class: B                    # docs/decisions.md (публичный контракт `single_instance`); согласие владельца в чате (2026-10-26)
files: [crates/pets-shell/src/lib.rs, crates/pets-shell/src/logging.rs, crates/pets-shell/LOGGING.md, crates/pets-shell/src/sys/, docs/decisions.md]
read: [Postman docs/tasks/PAR-AUDIT.md №21, 37, 39, 42; Postman ADR 0011 «Договор платформенной функции»]
checks: [fmt, clippy -p pets-shell linux/macos/windows, nextest -p pets-shell]
manual: yes                 # шаги — ниже

> ADR Postman 0011: одно поведение на всех ОС, отличие — только с причиной.

## Сделать
- №21 `credentials::write` — замена за один шаг, прежний секрет цел при сбое (как `CredWriteW`): macOS — `SecItemUpdate`
  (секрет и пользователь), записи нет — `SecItemAdd`; Linux — `CreateItem` с `replace=true` (oo7), записи того же имени с
  другим пользователем удаляются только после успешной записи.
- №37 системный журнал (`SystemLog`, `system_event`) — тот же уровень (INFO+) и коды: macOS — os_log (`_os_log_impl`,
  подсистема — имя приложения, категории `error`/`warning`/`info`), Linux — journald по родному протоколу через
  `/run/systemd/journal/socket` (без новых зависимостей; `PRIORITY`, `SYSLOG_IDENTIFIER`, `PETS_EVENT_ID`).
  Отличие с причиной: у os_log нет «предупреждения», тип `info` не хранится → WARN/INFO — `default` в своей категории.
- №39 `console::stderr_to_file` — одно правило: stderr в никуда (закрыт / null-устройство: `/dev/null`, на Windows — нет
  описателя или `NUL`) → файл; терминал, файл, канал, сокет — не трогаем.
- №42 единственный экземпляр: без `$XDG_RUNTIME_DIR` (macOS — `$TMPDIR`) — свой каталог `<tmp>/pets-<uid>` 0700 с
  проверкой владельца и прав, не общий `/tmp`; ошибка замка/каталога → `Err` (приложение — `fatal`, не второе окно).

## Приёмка
- nextest: `journald_datagram_fields`, `journald_report_without_panic`, `private_dir_is_owned_and_closed` (Linux);
  `oslog_report_without_panic` (macOS); прежние тесты экземпляра и консоли.
- clippy `-p pets-shell --all-targets` для x86_64-unknown-linux-gnu, aarch64-apple-darwin, x86_64-pc-windows-msvc — чисто.

## Ручная проверка
1. macOS: запуск Postman → `log show --last 5m --predicate 'subsystem == "Postman"'` — запись INFO о запуске (категория
   `info`); ошибка входа — категория `error`.
2. Linux (systemd): `journalctl -t Postman --since -5m -o verbose` — `PRIORITY=6`, `PETS_EVENT_ID=1004`.
3. Пароль: сменить пароль учётной записи (macOS Связка ключей / Linux Seahorse) — запись одна, новый пароль; при отказе
   связки (отмена разблокировки) — старый пароль читается.
4. Linux: `env -u XDG_RUNTIME_DIR postman` дважды — второй передаёт аргументы первому; `ls -ld /tmp/pets-$(id -u)` — `drwx------`.
   `chmod 755 /tmp/pets-$(id -u)` → запуск сообщает ошибку и не открывает окно.
5. Запуск из `.desktop`/Finder (stderr → `/dev/null`) и на Windows с `2> NUL` — журнал пишется в файл; `2> log.txt` — в log.txt.
