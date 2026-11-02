# Темы оформления: роли, плотность, «как в ОС», TOML, политика

**Применимость:** любое приложение на gpui (Windows; на других ОС — светлая Fluent без слежения).

**Проблема.** Единый стиль всех приложений Pets (базис, решение 2026-10-20): цвета не литералами, а ролями;
светлая/тёмная/высокая контрастность и акцент — из Windows и на лету; свои темы пользователя файлом без
рестарта; администратор может задать тему политикой.

**Решение.** `pets_ui::theme`. Роли и метрики — имена и значения токенов Урфина (`crates/ui/src/tokens`),
формат переопределений — его `[tokens.light]` / `[tokens.dark]`.

## Подключение

```rust
// main: до первого окна (чтение реестра — микросекунды; папку тем приложение читает там же, где конфиг).
let dir = profile.join("themes");
let (user_themes, errors) = pets_ui::theme::load_dir(&dir, APP_ROLES);
pets_ui::theme::Themes::init(ThemeConfig {
    app: "Postman",                 // ключ политики HKLM\Software\Policies\Pets\Postman
    choice: cfg.theme.clone(),      // "system" | id
    overrides: cfg.tokens.clone(),  // ThemeSpec из своего конфига или None
    user_dir: Some(dir),
    user_themes,
    app_roles: APP_ROLES,
    ..ThemeConfig::default()
}, cx);
// у каждого окна: вид окна gpui (светлая ↔ тёмная) — сразу.
Themes::observe_window(window).detach();

// render: только роли.
let t = Theme::global(cx);
div().bg(t.c(Role::LayerPanel)).text_color(t.c(Role::TextPrimary)).h(t.px(Metric::HRow))
```

Перерисовка: изменилась итоговая тема → `cx.refresh_windows()` (обходит кэш `.cached` view); на сам `Global`
можно подписаться `cx.observe_global::<Themes>(…)` (например, тост «тема с ошибками» по `Themes::errors`).

## Роли

| Роль базиса | Токен | Что |
|---|---|---|
| frame | `bg.base`, `bg.base.alt` | фон каркаса — сплошной, без Mica |
| panel | `layer.panel`, `layer.overlay` | карточки, оверлеи и тосты |
| text, secondary | `text.primary`, `text.secondary`, `text.tertiary`, `text.disabled` | текст |
| accent | `accent.fill(.hover/.pressed)`, `accent.on`, `accent.text`, `accent.subtle` | акцент ОС; `accent.on` — текст на заливке |
| cursor, cursor-text | `fill.cursor`, `fill.cursor.passive` + `text.primary` | курсор списка (активная/неактивная панель) |
| marked | `accent.text` на `accent.subtle` | помеченные строки |
| border | `stroke.card`, `stroke.control(.bottom)`, `stroke.strong`, `stroke.divider` | рамки и разделители |
| error, warning, success | `critical.fg/.bg`, `caution.fg/.bg`, `success.fg/.bg`, `info.bg` | состояния |
| — | `fill.subtle.hover/.pressed`, `fill.control`, `fill.control.input`, `shadow.overlay` | наведение, контролы, тень |

Роли приложения — `app.*` (`AppRole`: светлая, тёмная и роль pets-ui для высокой контрастности), например
типы файлов Урфина (`app.type.archive`) или цвета меток Postman (`app.label.blue`). Своей палитры приложение
не вводит: только роли поверх общей.

## Слои и режимы

1. Fluent светлая или тёмная (WinUI 3 Common theme resources) — все роли.
2. Акцент: ОС (`AccentPalette` из реестра, иначе `DWM\AccentColor` → оттенки OKLCH) или `accent = "#…"`;
   `accent.text` сдвигается до контраста 4,5:1 на `accent.subtle`, `accent.on` — белый или чёрный.
3. Таблица темы для её варианта; 4. переопределения пользователя. Чего нет в слое — из слоя ниже.

Встроенные: `system` («как в ОС»), `fluent-light`, `fluent-dark`, `high-contrast`. Включённая в ОС высокая
контрастность сильнее любой темы, переопределений и политики: только системные цвета (`GetSysColor`), без
полупрозрачности. Плотность (`density` = 20 / 24 / 28 / 32, по умолчанию 24; 32 — «стандарт» Урфина: строка выше, отступы как при 24) меняет высоты строк и контролов и
шкалу `space.*` (верх шкалы = высота строки); заголовок окна — 40 px всегда.

## Файл темы

`%APPDATA%\<App>\themes\<id>.toml`, id — имя файла (`[a-z0-9-]`); правка файла применяется на лету.

```toml
name = "Моя тема"
appearance = "dark"        # только тёмная; нет ключа — светлая/тёмная как в ОС
accent = "system"          # или "#RRGGBB"
density = 24
[tokens.dark]
"text.primary" = "#D2D2D2"
"app.label.blue" = "#7DB8E8"
```

Неизвестные ключи и роли — ошибка со списком (`ThemeError::Invalid { issues }`), тема не грузится, остальные —
да; ошибки — в журнал и `Themes::errors`. `Theme::to_spec(id).to_toml()` — «сохранить текущую как свою».

## Как в ОС на лету

- Вид окна gpui (`observe_window_appearance`) сообщает только смену светлая ↔ тёмная: `ImmersiveColorSet`
  с тем же видом (смена акцента) колбэк не зовёт, высокая контрастность приходит другим `WM_SETTINGCHANGE`.
- Поэтому поток `pets-ui-theme`: `RegNotifyChangeKeyValue` на `Themes\Personalize`, `Explorer\Accent`, `DWM`,
  `Accessibility\HighContrast` и `FindFirstChangeNotificationW` на папку тем — без опроса, 0 % CPU в простое.
  Событие → `futures::channel::mpsc` → задача gpui → `Themes`.

## Политика

`HKLM\Software\Policies\Pets\<App>` (и `HKCU\…` — пользовательская политика ADMX; HKLM сильнее):

| Значение | Тип | Смысл |
|---|---|---|
| `Theme` | `REG_SZ` | id темы; задана — выбор и цвета пользователя не действуют |
| `AllowThemeChange` | `REG_DWORD` | `0` — смену темы запретить (без `Theme` — «как в ОС»); `1` — можно |

Плотность и высокая контрастность ОС действуют и под политикой. `ThemeState::locked()` — выбор темы в
настройках показывать только для чтения с подписью `pets-ui-theme-locked`.

**Откуда.** Токены и акцент — Урфин (S2 §1.2–1.5, `tokens`, `accent.rs`, `sys/windows.rs`); pets-ui,
2026-10-21.
