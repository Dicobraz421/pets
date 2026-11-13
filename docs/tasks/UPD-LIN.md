# UPD-LIN: pets-update на Linux — AppImage, deb/rpm ссылкой (паритет Postman)

lane: update
status: done
deps: []
class: B                    # docs/decisions.md (публичный API)
files: [crates/pets-update/src/product.rs, crates/pets-update/src/install.rs, crates/pets-update/src/install/tests.rs, crates/pets-update/README.md, docs/decisions.md]
read: [Postman docs/decisions/2026-11-12-linux.md, Postman DEV.md «Выпуск»]
checks: [fmt, clippy -p pets-update linux + x86_64-pc-windows-gnu, nextest -p pets-update]
manual: yes

> Postman PR #627: выпуск `postman-<версия>-linux-x64.{AppImage,deb,rpm}` с `.minisig`.

## Сделать
- `InstallKind::AppImage`: `.AppImage` + `.minisig`, подпись — тем же путём; запись рядом с `$APPIMAGE` во временный,
  `0o755`, атомарное переименование; ошибка — временный удалён, старый на месте. Перезапуск — помощником.
- `InstallKind::{Deb, Rpm}`: `can_install() == false`, итог «доступна версия X» + «Скачать» (`asset_url`).
- `InstallKind::detect_linux`/`current_linux`: `$APPIMAGE` → AppImage; exe под `/usr` → пакет (dpkg/rpm).

## Приёмка
- Тесты: `linux_kind_is_detected`, `linux_asset_names`, `appimage_*` (замена и права, подмена, откат, остаток), `packages_are_not_installed`.

## Ручная проверка (Linux x64, выпуск Postman с подписями)
1. AppImage старой версии в `~/Apps` → «Установить» → программа закрылась и открылась новой версией; файл тот же,
   исполняемый, рядом нет `.Postman*.pets-update-new`.
2. Папка образа только для чтения (`chmod a-w ~/Apps`) → ошибка до выхода, старая версия работает.
3. Установленный deb (`/usr/bin/postman`) → «Доступна версия X» и «Скачать» ведёт на `.deb`; на Fedora — на `.rpm`.
