# Решения владельца

Новое решение — сверху, с датой.

## 2026-10-28. Платформенный код приложений — в pets-gpui-windows и pets-shell (Postman, T1-18)

Решение владельца: «всё платформозависимое уноси в выделенные модули»: окно — `pets-gpui-windows`, без UI —
`pets-shell`. Приложение не держит `windows`/`windows-sys` и `unsafe` для окна и оболочки ОС. API добавлено, прежнее
не менялось. `gpui_windows::pets` (Pets patch; ошибки — `std::io::Error`):
- `drag`: `DragFile::bytes(name, Arc<Vec<u8>>)` / `DragFile::lazy(name, size, provider)` / `.with_copy(path)`,
  `drag_out(files) -> Outcome { Dropped, Cancelled }` (OLE: виртуальные файлы + запасной `CF_HDROP`), `threshold(scale)`,
  `TempCopies::new(app)` (`%TEMP%\<app>\drag`: `slot`, `clean`). Что тащить и как писать копию — решает приложение.
- `window`: `id(&Window)`, `set_icons(&Window, resource)`, `icon_sizes(resource)`, `capture(id) -> Image`.
- `toast::show(&Sender { aumid, name, icon }, title, body)` — без AUMID процесса (группировка на панели задач прежняя).
`pets_shell` (только Windows, `unsafe` — в `sys`; ошибки — `ShellError`, новый вариант `Invalid`):
- `registry` (HKCU запись, HKCU/HKLM чтение), `credentials` (диспетчер учётных данных), `console` (`attach_parent`,
  `stderr_to_file`, `active_code_page`), `memory::report` (бенч).

## 2026-10-21. pets-commands: схемы клавиш (Postman S5-17)

Несколько наборов сочетаний у одной таблицы команд — «как Outlook» по умолчанию и «одиночные клавиши как Gmail».
API (добавлено, прежнее не менялось):
- `Scheme { id, keys: &[SchemeKeys] }`, `SchemeKeys { command, keys, context }` — слой сочетаний схемы со своим
  контекстом (одиночные буквы — «вне полей ввода»); клавиши по умолчанию — по-прежнему `Command::keys`.
- `bindings(commands, scheme, same_place) -> Vec<Binding>` — итоговые привязки: по умолчанию, затем слой;
  сочетание по умолчанию, которое слой дал там же (`same_place` решает приложение), уходит. `Binding { command,
  keys, context, from_scheme }`.
- `validate_scheme(commands, scheme)` — команды есть, сочетания разобраны, в слое без повторов в одном контексте;
  `Problem::UnknownCommand`.
- Свои клавиши пользователя — в приложении поверх `bindings`. Потребитель — Postman (Урфин pets-commands пока не
  подключает).

## 2026-10-21. pets-ui: темы оформления (Postman S5-01, базис «единый стиль»)

Модуль `pets_ui::theme` — общие токены и темы всех приложений Pets. API:
- `Role` (33 роли), `Metric` (33 метрики), `Density` (`Compact` 20 / `Normal` 24 / `Comfortable` 28 / `Spacious` 32 px),
  `FontSizes` — **имена и значения как у токенов Урфина** (`crates/ui/src/tokens`): тест сверяет таблицу
  ролей и метрик со снимком Урфина для акцента #0078D4. Цвет в gpui — `Hsla`; `Color` — sRGB 8 бит для TOML.
- `Theme` (`c(Role)`, `m(Metric)`, `px(Metric)`, `app("app.…")`, `appearance`, `density`, `to_spec(id)`),
  `Theme::global(cx)`; `contrast`, `over`, `MIN_CONTRAST`.
- `ThemeSpec` — TOML темы и переопределений: `name`, `appearance`, `accent`, `density`, `[tokens.light]` /
  `[tokens.dark]` с ролями-ключами (формат `[ui.tokens.*]` Урфина); `ThemeSpec::parse` / `to_toml`;
  неизвестные роли и ключи — `ThemeError::Invalid { issues }` со списком, недостающие роли — из базовой.
- `resolve(spec, user, os, fonts, app_roles)` — слои Fluent → акцент → тема → пользователь; высокая
  контрастность ОС сильнее всего.
- Встроенные: `system` («как в ОС»), `fluent-light`, `fluent-dark`, `high-contrast`. `OsLook::read()` —
  `AppsUseLightTheme`, `SPI_GETHIGHCONTRAST` + `GetSysColor`, `AccentPalette` / `DWM\AccentColor`.
- `Themes` — `Global` gpui: `Themes::init(ThemeConfig, cx)`, `Themes::update`, `Themes::set_choice`,
  `Themes::observe_window(window)`; поток `pets-ui-theme` следит за реестром и папкой тем (горячая
  перезагрузка); `ThemeState` — то же без gpui; `load_dir`, `title`.
- Роли приложений — `AppRole` в пространстве `app.*` (светлая, тёмная, роль pets-ui для высокой контрастности).
  Роли типов файлов Урфина (`type.*`) и цвета Postman (категории, метки, срок, угроза) — роли приложений.
- Политика `HKLM\Software\Policies\Pets\<App>` (и HKCU): `Theme` (REG_SZ) — тема задана, `AllowThemeChange`
  (REG_DWORD 0/1); `Policy::read`, `ThemeState::locked`.

`Toasts`: кнопка действия — `push_action(level, text, ToastAction, cx)`, тост с действием виден 15 с
(`ACTION_DURATION`); `Queue::push_with`, `Queue::take_action`; цвета — роли темы. `Toasts::push` и `Queue::push`
прежние; у `Toast` новое поле `action` — несовместимо только для того, кто собирает `Toast` литералом (Postman —
нет). Метка предупреждения — нейтральная (`text.secondary`): красный — только ошибки (Postman DESIGN).

Открыто:
- **TODO (сессия Урфина):** темы **Графит** и **Navigator** (и необязательная **Бумага**) — значения ролей
  у Урфина не заданы; добавить файлами `ThemeSpec` во встроенные, когда Урфин их определит (Navigator — моно-шрифт
  во всём UI: метрика/флаг шрифта, в API тем его пока нет).
- Плотность: базис 20 / 24 / 28 плюс 32 (`Spacious`) — «стандарт» Урфина: строка и контролы 32, отступы как при 24
  (ревью Урфина). В TOML — числом (`density = 32`); Урфину при переходе: `compact` → 24, `standard` → 32.
- Индикатор контекста (полоса заголовка от администратора / в удалённой сессии) — не в этой задаче.

## 2026-10-20. pets-i18n, pets-shell, pets-ui (Postman S2-06)

Первые версии крейтов базиса, потребитель — Postman. API:
- `pets-i18n`: `init(&[&[Resource]], lang)`, `tr!`, `set_language`, `epoch`; `check` и `raw_strings` — для
  тестов приложения. Тексты `.ftl` — в приложении и в библиотеках (`pets_ui::FTL`), язык `auto` — язык ОС.
- `pets-shell`: `init_logging(app, env_var)` (ERROR и паники — ещё и в журнал Windows «Приложение»), `fatal`,
  `single_instance(app, args)` → `First(Listener)` / `Second`.
- `pets-ui`: `Toasts` (view gpui) — тосты справа сверху без фокуса, история панелью; уровни `Level`.
- Журнал (`tracing`) пишется по-английски; по-русски — только тексты интерфейса через Fluent.

## 2026-10-19. Патч gpui в pets

Патч Windows-платформы gpui из Postman (`third_party/gpui-pre-windows`) — в `crates/pets-gpui-windows`.
Имя пакета прежнее (`gpui-pre-windows`, Apache-2.0, снимок Zed) — для `[patch]`; каталог вне рабочей
области `pets`, со своим `[workspace]` и `Cargo.lock`; CI — отдельная задача с тестами наших правок.

## 2026-10-19. Репозиторий pets, gpui для всех

1. Интерфейс всех приложений Pets — gpui (gpui-kit / gpui-pre с нашими патчами). egui отвергнут и в общий
   код не попадает. Урфин остаётся на egui до отдельного решения о переходе — в его собственном процессе.
2. Общий код — репозиторий `pets`, по функциональным подходам: `pets-gpui-windows` (платформа gpui),
   `pets-commands` (реестр команд), `pets-i18n` (тексты), `pets-shell` (жизнь процесса), `pets-ui`
   (визуальный язык на gpui). Крейт создаётся, только когда есть потребитель.
3. Специфика приложений (почта, файловый менеджер) не смешивается: общее — подходы и механизмы,
   предметное — в приложении. Знания без кода — рецепты в `docs/recipes/` с пометкой применимости.
4. Приложения подключают `pets` git-зависимостью с фиксированной ревизией.
