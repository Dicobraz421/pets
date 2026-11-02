# Урфин — перевод на pets-dev

Исполнитель — Claude Code на стенде. Порядок R-01 → R-07; R-01 и R-02 — класс B (approve владельца),
дальше — класс A. Ветки `x/rollout-NN`. Каждый пункт — один PR. Шаблоны — `pets/skills/pets-dev/templates/`.

## R-01 CI по шаблону (≤7 мин на PR и в очереди)
- `.github/workflows/ci.yml` из шаблона; сохранить: `pr-title`, `deny`, профиль nextest `ci`,
  `URFIN_TEST_RECYCLE=1`, `ci/release_features.py`, `ci/size.py` (30 МиБ), `--smoke`, артефакт `urfin-Windows`.
- Release-шаги — в job `release` (main и бета); в `merge_group` release нет.
- rust-cache: `shared-key: windows`, `save-if` только main (merge queue сейчас 19 мин — холодный кэш).
- `gate` — единственный required check (владелец меняет в ruleset, `rollout/github-settings.md`).
- `lane_check.py` → `.github/`, `lanes.toml` и `CODEOWNERS` в корень (полосы по крейтам: vfs, ops, term, core,
  ui+app, view, i18n, docs, docs-gen, deps, ci).
Приёмка: docs-PR — `gate` зелёный без сборки; код-PR ≤7 мин при тёплом кэше.

## R-02 CLAUDE.md ≤3 КБ
- По шаблону: без «текущего пакета» и списков задач; запуск, стенд, экономия сборок — в `docs/DEV.md`.

## R-03 Ручные проверки — файл на задачу
- `docs/MANUAL-TESTS.md` (130 КБ) → `docs/manual-tests/ARCHIVE.md` без изменений, больше не правится.
- Новые — `docs/manual-tests/<ID>.md`; статус разделов — генерируется тестом/скриптом, не правится руками.

## R-04 Решения — файл на решение
- `docs/specs/S0-decisions.md` заморозить (архив); новые — `docs/decisions/<дата>-<slug>.md`;
  индекс `docs/decisions/README.md` генерирует скрипт. Ссылки вида «№93» в коде/доках оставить (архив).

## R-05 Один текущий контракт
- `S0-contracts-v2…v6` → `docs/CONTRACTS.md` (действующее состояние) + раздел «Изменения»; версии — `docs/archive/`.

## R-06 Строки и реестры без общих файлов
- `crates/i18n/locales/*/ui.ftl` → файл на часть экрана (как Postman, список в коде).
- `COMMANDS.md`: тест `commands_doc_is_current` не блокирует PR; обновление — полоса `docs-gen` раз в день.
- `gpui_shell/mod.rs`, `app/src/main.rs` (горячие файлы): разведка — какие правки повторяются; предложение
  регистрации в модуле фичи — ADR, затем волна.

## R-07 Карточки задач
- Открытые задачи из `docs/TASKS-*.md` → `docs/tasks/<ID>.md` по шаблону (поле `lane`, `files`, `read`).
  Старые TASKS-файлы — индекс со ссылками.
