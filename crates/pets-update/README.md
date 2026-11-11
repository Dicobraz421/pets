# pets-update

Обновление приложений Pets — один механизм для Урфина и Postman (решение владельца 2026-11-11,
`docs/decisions/2026-11-11-pets-update.md`). Без UI: уведомление, настройки и кнопки рисует продукт.

Что делает:

- **Проверка.** `GET https://api.github.com/repos/<repo>/releases` — раз в сутки и по кнопке. В запросе только имя
  репозитория и `User-Agent: pets-update`: ни версии, ни идентификаторов, ни данных пользователя.
- **Выбор.** Самый новый выпуск строго новее текущей версии (semver; откат не предлагается), с учётом канала:
  стабильный не видит версий с дефисом (`-beta.N`, `-rc.N`). Выпуск без артефакта или подписи пропускается.
- **Скачивание** — только после согласия пользователя: артефакт и `<артефакт>.minisig`, с прогрессом и отменой.
- **Подпись.** minisign (ed25519 по хэшу BLAKE2b) ключом, вшитым в программу. Подпись привязана и к имени файла
  (`file:<имя>` в подписанном комментарии): старый установщик или файл другого продукта под видом нового не пройдёт.
  Файл с неверной подписью удаляется и не запускается.
- **Установка.** MSI — `msiexec /i <файл> /passive /norestart`; portable — замена файлов рядом с exe с откатом.
  Потом программа запускается снова.
- **Выключатели.** Настройка выключена — фоновых запросов нет, кнопка «Проверить» работает. Политика
  `AllowUpdate = 0` — запросов нет вообще.

Сеть — только HTTPS, TLS и прокси системные (SChannel, настройки Windows), таймауты: соединение 15 с, ответ 30 с,
простой при чтении 30 с. Все методы блокирующие — звать из фонового исполнителя, не из UI-потока.

## Подключение в продукте

```toml
pets-update = { git = "https://github.com/Dicobraz421/pets", rev = "<коммит>" }
```

```rust
use pets_update::{CheckOutcome, CheckReason, InstallKind, InstallPlan, Product, Settings, Updater, Version};

fn product() -> Product {
    Product {
        name: "Urfin",                       // ключ политики: HKLM\Software\Policies\Aahz\Urfin
        repo: "Dicobraz421/Urfin",           // откуда брать выпуски
        asset_prefix: "urfin",               // urfin-<версия>-windows-x64.msi
        current: Version::parse(env!("CARGO_PKG_VERSION")).expect("version"),
        public_keys: &["RWQ…"],              // строка из .pub владельца; вторая — на время смены ключа
    }
}

fn main() {
    // 1. Первая строка main: это мог быть запуск помощника установки.
    if let Some(code) = pets_update::run_helper(&product()) {
        std::process::exit(code);
    }
    // 2. При старте: итог прошлой установки → тост «Обновлено до X» / «Не удалось обновить: …».
    let dir = profile_dir().join("updates");
    if let Some(result) = pets_update::take_result(&product(), &dir) { /* показать */ }

    let kind = InstallKind::detect(exe_dir, "urfin.portable");
    let updater = Updater::new(product(), kind, dir);   // читает политику; сети нет
    // 3. В фоне: через минуту после старта и дальше раз в час. Крейт сам решает, пора ли (раз в сутки,
    //    после ошибки — через час), и сам помнит время проверки в `updates/state.json`.
    match updater.check(&settings, CheckReason::Scheduled)? {
        CheckOutcome::Available(update) => { /* тост «Вышла версия {update.version} · Установить» */ }
        _ => {}
    }
    // Кнопка «Проверить» — CheckReason::Manual.
}

// 4. Пользователь нажал «Установить» (фоновая задача):
let downloaded = updater.download(&update, &cancel, &mut |p| { /* p.received / p.total → UI */ })?;
let verified = updater.verify(downloaded)?;              // неверная подпись → Err, файл удалён
updater.install(verified, InstallPlan::restart_current()?)?;
// 5. Сразу закрыть программу (обычный выход с сохранением сессии): помощник ждёт её завершения.
```

Что учесть:

- `Settings { enabled, channel }` хранит продукт; по умолчанию включено, канал стабильный.
- `updater.policy().updates_allowed() == false` — выключатель в настройках показать запертым, кнопку «Проверить»
  спрятать; `check` вернёт `Disabled(DisabledBy::Policy)`, `download` — `Err(DisabledByPolicy)`.
- Ошибки `check` по расписанию — в журнал, пользователю не показывать; по кнопке — показать.
- Portable: в `InstallPlan::keep` перечислить файлы рядом с exe, которые нельзя перезаписывать (файл-флаг,
  настройки). Файлы, которых нет в новом архиве, остаются на месте.
- Exe собран со статическим CRT (`-C target-feature=+crt-static`): помощник — копия exe в папке обновления и должен
  запускаться без соседних DLL. `run_helper` стоит до любого кода, которому нужны файлы из папки программы.
- MSI: постоянный `UpgradeCode` и `MajorUpgrade`, установка в ту же папку. Пакет «для всех» сам покажет запрос UAC.
- Тесты продукта: `Updater::with_parts(product, kind, dir, policy, Box::new(подменная_сеть))`.

Как это работает внутри: `install` копирует exe программы в `updates/<prefix>-update-helper.exe` и запускает копию
с `--pets-update-apply <job.json>`. Помощник ждёт выхода программы, **заново проверяет подпись** и держит файл
закрытым от записи, пока идёт установка, пишет `result.json`, удаляет скачанное и запускает программу.

Проба на стенде (настоящая сеть и настоящий помощник, без окон): `cargo run -p pets-update --example probe`.

## Групповая политика

`HKLM\Software\Policies\Aahz\<Продукт>` (и тот же путь в HKCU для пользовательских политик; HKLM сильнее):

| Значение | Тип | Смысл |
|---|---|---|
| `AllowUpdate` | `REG_DWORD` | `0` — обновление выключено, сетевых запросов нет; `1` или нет значения — как в настройках |

## Имена артефактов выпуска

Как в `skills/pets-dev/references/releases.md`, плюс подписи:

| Файл | Что |
|---|---|
| `<prefix>-<версия>-windows-<arch>.msi` | установщик; `arch` — `x64` или `arm64` |
| `<prefix>-<версия>-windows-<arch>.zip` | portable, файлы в архиве лежат сразу, без вложенной папки |
| `<артефакт>.minisig` | подпись каждого `.msi` и `.zip` |

Тег выпуска — `v<версия>`, версия — semver; версия с дефисом — предварительный выпуск (бета-канал). Имя файла
должно совпадать с этим шаблоном буква в букву: по нему ищется артефакт и к нему привязана подпись. Выпуск, в
котором нет `.minisig`, программа не предлагает — старые выпуски без подписей обновлению не мешают.

## Подпись в release-workflow

Ключ создаёт и хранит владелец. В репозиторий, в журналы CI и в чат закрытый ключ не попадает.

**Один раз (владелец, на своей машине).** Поставить [minisign](https://github.com/jedisct1/minisign/releases) и
создать пару без пароля (защита ключа — хранилище секретов GitHub):

```
minisign -G -W -p pets-update.pub -s pets-update.key
```

- содержимое `pets-update.key` — в секрет `UPDATE_SIGNING_KEY` репозитория продукта (Settings → Secrets and
  variables → Actions) или организации; сам файл — в менеджер паролей, с диска удалить;
- вторую строку `pets-update.pub` (начинается с `RW`) — в `Product::public_keys` продукта. Открытый ключ не секрет.

Один ключ на оба продукта допустим: подпись привязана к имени файла, а оно начинается с имени продукта.

**В job `publish`** (ubuntu) — перед расчётом `SHA256SUMS.txt`, чтобы подписи попали и в суммы, и в выпуск:

```yaml
      - name: Подпись обновления (minisign)
        shell: bash
        env:
          UPDATE_SIGNING_KEY: ${{ secrets.UPDATE_SIGNING_KEY }}
          UPDATE_PUBLIC_KEY: ${{ vars.UPDATE_PUBLIC_KEY }}   # та же строка RW…, что вшита в программу
        run: |
          if [ -z "$UPDATE_SIGNING_KEY" ]; then echo "::error::нет секрета UPDATE_SIGNING_KEY"; exit 1; fi
          sudo apt-get install -y minisign >/dev/null
          key="$RUNNER_TEMP/update.key"
          (umask 077; printf '%s\n' "$UPDATE_SIGNING_KEY" > "$key")
          trap 'rm -f "$key"' EXIT
          cd assets
          for f in *.msi *.zip; do
            minisign -S -s "$key" -m "$f"            # пишет $f.minisig; в подписи — file:$f
            minisign -V -P "$UPDATE_PUBLIC_KEY" -m "$f"
          done
```

- Подписывается готовый файл — после SignPath/Authenticode, если она есть: любая правка файла после minisign
  ломает подпись.
- Комментарий подписи не менять (`-t` не задавать): крейт ищет в нём `file:<имя файла>`, minisign пишет его сам.
- Выпуск без секрета должен быть красным, а не «без подписи»: иначе пользователи его просто не увидят.

**Смена ключа.** Выпустить версию с двумя ключами в `public_keys` (старый и новый), подписанную старым; со
следующей подписывать новым; старый убрать, когда пользователи обновятся. Ключ утёк — то же самое срочно, плюс
удалить выпуски, подписанные после утечки.

## Чего здесь нет

- UI, тексты, хранение настроек — в продукте.
- Тихой установки без согласия и скачивания «заранее».
- Разностных обновлений и докачки: файл качается целиком.
- Удаления файлов portable, пропавших из новой версии.
