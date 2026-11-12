# Решения владельца

Новое решение — сверху, с датой.

## 2026-11-12. gpui-kit 0.9.1 (gpui-pre 0.3.12) во всех крейтах pets (GKIT-01)

Владелец (класс B согласован в чате): Pets и Postman — с gpui-kit 0.7.3 / gpui-pre =0.3.9 на 0.9.1 / =0.3.12.
`pets-gpui-base` — снимок `gpui-base` 0.9.1, `pets-gpui-windows` — `gpui-pre-windows` 0.3.12, оба с прежними патчами
(таблицы в их README). Публичное API крейтов pets не менялось; потребитель берёт все крейты с одного коммита и ставит
`gpui-kit = "=0.9.1"` у себя. Шейдер `emoji_rasterization` убран вслед за upstream (эмодзи растрируются на CPU).
## 2026-11-12. Linux: pets-shell с API macOS (SHELL-LIN-01, Postman Linux tier 2)

Владелец (Postman `docs/decisions/2026-11-12-linux.md`: Linux tier 2, стандарты freedesktop, без Flatpak). API
добавлено для `target_os = "linux"`, прежнее не менялось; Windows и macOS по поведению не меняются:
- `single_instance` — как на macOS (`flock` + unix-сокет), каталог `$XDG_RUNTIME_DIR` (нет — `temp_dir`, WARN); код
  macOS перенесён в `sys/unix.rs`, `console_macos.rs` → `console_unix.rs`.
- `pets_shell::credentials::{write, read, delete}` — Secret Service через `oo7` 0.6, только D-Bus-бэкенд (`oo7::dbus::Service::encrypted` — зашифрованный сеанс; не
  `oo7::Keyring`: в песочнице он уходит в файл), фичи как у gpui-pre-linux (`async-std`, `native_crypto`) — новых
  пакетов у Postman нет;
  коллекция `default`, атрибуты `xdg:schema=org.pets.Credential`, `service`, `username`. Нет шины или службы —
  `ShellError::Os` с `io::ErrorKind::NotFound`; на диск секрет не пишется. Заблокированная связка — запрос службы.
- `pets_shell::console::stderr_to_file` — stderr закрыт или `/dev/null` → файл; поток в journald не трогаем.
- `pets_shell::url_scheme::{bundle_id, set_default, default_handler, is_default}` — `bundle_id` = id `.desktop`, чей
  `Exec` — этот исполняемый файл (`XDG_DATA_HOME`/`XDG_DATA_DIRS`); `set_default` — `xdg-settings`, запасной путь —
  `xdg-mime default`; нет `.desktop` — ошибка (как вне `.app`). `.desktop` с `MimeType=x-scheme-handler/mailto;` —
  в пакете приложения.
- `pets_shell::login_item::{State, state, set, open_settings}` — XDG Autostart `~/.config/autostart/<id>.desktop`;
  выключенная средой запись (`Hidden`, `X-GNOME-Autostart-enabled=false`) — `RequiresApproval`; `open_settings` —
  каталог `autostart` через `xdg-open`.
- `pets_shell::memory::phys_footprint()` — `Private_Clean + Private_Dirty + SwapPss` из `/proc/self/smaps_rollup`.
- `pets_shell::clipboard::{has_html, read_html}` — `text/html` через `wl-paste` (Wayland) или `xclip` (X11), таймаут
  2 с; UTF-16 с BOM (Firefox) декодируется; разбор — `parse_mac_html`. `write_html` на Linux — ошибка `Invalid`.
- Зависимости Linux: `oo7` 0.6 и `zbus` 5.19 (`default-features = false`, `async-io`; не 6.x — вторая копия рядом с zbus 5 от gpui раздула бы Postman, решение оркестратора 2026-11-12), `libc` 0.2.191; Cargo.lock — класс B.

## 2026-11-12. macOS: бейдж на значке в Dock (Postman MAC-07)

Владелец (Postman, волна MAC — класс B, согласована): число непрочитанных на значке в Dock — как подсказка значка в
трее на Windows. API добавлено, прежнее не менялось: `pets_gpui_macos::dock::set_badge(Option<u32>)` (0 и `None` —
без бейджа) и `set_badge_label(Option<&str>)` — свой текст («999+»); только из главного потока, иначе ошибка.

## 2026-11-12. Без названий ОС в текстах интерфейса (Postman MAC-04)

Владелец (Postman `docs/decisions/2026-11-12-no-os-names.md`): в текстах интерфейса на всех ОС нет названий ОС и
систем, кроме явной необходимости. Системная тема `pets-ui` — «Как в системе» / «System» на всех ОС; ключ
`pets-ui-theme-system-mac` (pets#55) убран.

## 2026-11-12. macOS: буфер HTML, снимок ScreenCaptureKit, тема, объект входа, политики (Postman MAC-04)

Владелец (Postman `docs/decisions/2026-11-11-macos.md`, волна MAC-04 — класс B, одобрена 2026-11-12). API добавлено,
прежнее не менялось; Windows не меняется:
- `pets_shell::clipboard` на macOS — `public.html` (`NSPasteboardTypeHTML`) и текст: `read_html`, `has_html`,
  `write_html` работают и там; без ОС — `parse_mac_html(&str)`, `encode_mac_html(fragment)` (документ без
  заголовка `CF_HTML`: Word — с отметками фрагмента, Safari — `<meta charset>` без `<body>`).
- `pets_shell::login_item` (macOS 13+): `state()`, `set(on)`, `open_settings()` — `SMAppService.mainApp`.
- `pets_shell::managed::get(key)` (macOS): принудительное значение профиля конфигурации для пакета приложения —
  аналог `Software\Policies` (свои `defaults write` пользователя не считаются).
- `pets_shell::memory::phys_footprint()` (macOS) — «Память» Мониторинга системы, для бенча.
- `pets_gpui_macos::window::capture` — ScreenCaptureKit на macOS 14.4+ (окна своего процесса без разрешения «Запись
  экрана»), на 13 — 14.3 и при отказе — CoreGraphics; `toast::show_clickable(title, body, sound, on_click)`.
- `pets-ui` тема на macOS: светлая/тёмная — `AppleInterfaceStyle`, акцент — `NSColor.controlAccentColor`, политика
  `Theme`/`AllowThemeChange` — профиль конфигурации; смена — распределённые уведомления. Папка тем на macOS не
  отслеживается (новые файлы — со следующего запуска).
- Mac-job в CI pets нет: сборка — `cargo clippy --target aarch64-apple-darwin` (pets-ui — с gpui, только на маке),
  проверка на маке — CI `macOS` Postman; тесты pets для macOS (`unique_pasteboard_round_trip` на своём буфере,
  `footprint_is_reported`, …) — до mac-job pets (Postman MAC-06) не запускаются.

## 2026-11-12. pets-gpui-windows: значок в трее, спрятать окно, щелчок по уведомлению (Postman T3-13)

Владелец (Postman `docs/decisions/2026-11-12-tray-and-notifications.md`): закрытие окна сворачивает приложение в трей,
уведомления приходят и без окна, щелчок по уведомлению открывает окно. API добавлено, прежнее не менялось
(`gpui_windows::pets`):
- `tray::Tray::new(icon, tip, on_event)` — значок из ресурса exe, пока объект жив; `set_tip`, `set_icon`,
  `set_menu(Vec<Item>)`; события `Event::Click` и `Event::Menu(id)`. Значок возвращается после перезапуска
  Проводника. Обработчик — из оконной процедуры значка: только передать событие в задачу интерфейса.
- `window::hide`, `window::show`, `window::is_visible` — спрятать окно с экрана и панели задач и вернуть
  (асинхронно, без повторного входа в оконную процедуру gpui); вперёд — `Window::activate_window`.
- `toast::show_clickable(sender, title, body, sound, on_click)` — щелчок по уведомлению, пока процесс жив;
  `on_click` — из потока Windows.
- Потребитель — Postman; Урфину пригодится трей при фоновых операциях, пока не нужно.

## 2026-11-11. pets-shell: HTML в буфере обмена (Postman T3-06)

Владелец: «Не делай двойную работу, пиши буфер как следует» — чтение `CF_HTML` не в приложении, а здесь. Буфер обмена
gpui отдаёт только текст, картинки и файлы. API добавлено, прежнее не менялось (`pets_shell::clipboard`, все ОС; не
Windows — HTML нет):
- `read_html(max) -> Result<Option<Html>, ShellError>` — фрагмент `StartFragment…EndFragment` и `SourceURL`;
  `has_html()`; `write_html(fragment, text, source_url)` — `CF_HTML` и `CF_UNICODETEXT` разом (копирование с
  форматированием).
- Без ОС: `parse_cf_html(&[u8]) -> Option<Html>`, `encode_cf_html(fragment, source_url) -> Vec<u8>`, `DEFAULT_MAX`.
- HTML буфера — чужие данные: разбирает приложение (Postman — в воркере), здесь только байты. Потребитель — Postman
  (вставка таблиц из Word и браузера в письмо); Урфину не нужно.

## 2026-11-11. Патч gpui-kit в pets: блоки в тексте поля (Postman T3-06, T3-07)

Владелец (Postman#562, вариант А): таблицы в письме — сеткой, картинки — своего размера в тексте; поле gpui-kit этого
не умело (все строки одной высоты, своих элементов в тексте нет). Патч — в `crates/pets-gpui-base`: снимок `gpui-base`
0.7.3 (Apache-2.0) под прежним именем пакета, подмена через `[patch.crates-io]`, как `pets-gpui-windows`; каталог вне
рабочей области `pets`. API добавлено, прежнее не менялось: `EditorBlock`, `EditorBlockContext`, `EditorBlockRenderer`,
`EditorState::set_blocks` / `clear_blocks` — блоки как в Zed (отведённые строки кратны высоте строки). Кандидат в
upstream (longbridge/gpui-kit). Урфину не нужно (у него свой редактор).

## 2026-11-09. Звук уведомления Windows (Postman T2-10)

`gpui_windows::pets::toast::show_with(&Sender, title, body, Sound)` — со своим звуком: `Sound::Default` (как прежде),
`Mail` (звук «Новое письмо»), `Silent` (молча). `show` не менялся (`Sound::Default`). Потребитель — Postman
(«Настройки → Общие → Звук „Новое письмо“»); Урфин уведомлений не показывает.

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
