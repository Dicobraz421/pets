# Postman — перевод на pets-dev

Исполнитель — Claude Code на стенде. Порядок R-01 → R-06; R-01 и R-02 — класс B, дальше — класс A.
Ветки `x/rollout-NN`, PR на пункт. Шаблоны — `pets/skills/pets-dev/templates/`.

## R-01 CI по шаблону (сейчас 13–24 мин на PR и ещё раз на main)
- В PR и `merge_group`: метки конфликта, clippy, тесты (с правами AppContainer `icacls` и
  `POSTMAN_REQUIRE_FULL_SANDBOX=1`). Без release, подписи и архива.
- Release, подпись, `package.ps1`, артефакт `Postman-<версия>-windows-x64.zip` — job `release` (main, `beta-*`,
  `workflow_dispatch`). Диагностику песочницы — туда же или вручную.
- Добавить `merge_group`, `changes` (docs-PR без сборки), `gate`, rust-cache `shared-key`/`save-if` main,
  `cargo nextest` (профиль `ci`, retries 1). `lane_check.py`, `lanes.toml` (полосы по слоям: ir, протоколы,
  cache, worker+smime, core, backend-*, app-gpui, demo, docs, docs-gen, deps, ci), `CODEOWNERS`
  (+ `crates/worker/`, `crates/smime/` — песочница).
Приёмка: PR ≤7 мин при тёплом кэше; архив по-прежнему собирается на main.

## R-02 CLAUDE.md 13,6 КБ → ≤3 КБ
- По шаблону. «Запуск», «Снимки экранов», стенд Stalwart, диск — в `docs/DEV.md`; таблица слоёв — в
  `docs/00-ARCHITECTURE.md` (или ссылкой на `crates/core/tests/layers.rs`). «С чего начинать сессию» — убрать
  (текущее — в карточках). CLAUDE.md правился в 39 % PR — после R-02 только класс B.

## R-03 Решения — файл на решение
- `docs/specs/S0-decisions.md` заморозить; новые — `docs/decisions/<дата>-<slug>.md`, индекс генерируется.

## R-04 Генерируемые доки
- `PARITY.md`: тест не блокирует PR; обновление — полоса `docs-gen` раз в день.

## R-05 Горячие файлы app-gpui
- `main.rs`, `screens.rs`, `commands.rs`, `mailer.rs` (39–51 правка): разведка повторяющихся правок, ADR на
  регистрацию в модуле фичи (сцены `SCENES`, команды, тексты `TEXTS`), затем волна.

## R-06 Карточки задач
- Открытые задачи `docs/specs/S5.md…S7.md` → `docs/tasks/<ID>.md` с полями `lane`, `files`, `read`.
