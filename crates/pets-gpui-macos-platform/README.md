# pets-gpui-macos-platform

macOS-платформа gpui с нашими патчами. Пакет называется как оригинал — `gpui-pre-macos` 0.3.12 (снимок
`gpui_macos` из zed@d2a4fb6 с crates.io, Apache-2.0), чтобы подменять его через `[patch]`:

```toml
[patch.crates-io]
gpui-pre-macos = { git = "https://github.com/Dicobraz/pets", rev = "<коммит>" }
```

Не путать с `crates/pets-gpui-macos` — нейтральный API окна для приложений (аналог `gpui_windows::pets`).
Каталог исключён из рабочей области `pets` (чужой код со своим стилем и `Cargo.lock`). Правки помечены «Pets patch».

| Патч | Файлы | Upstream |
|---|---|---|
| Аварийный откат кадров на CVDisplayLink — `GPUI_CV_DISPLAY_LINK=1` (раз за процесс). По умолчанию — как в исходнике и как рекомендует Apple: `CAMetalDisplayLink` на macOS 14+, CVDisplayLink на macOS 13 (там Metal-ссылки нет) | `src/window.rs`, `src/gpui_macos.rs` | — |
| Диагностика кадров (GMAC-01, цель `gpui_macos::frames`, сторож пауз) — см. ниже | `src/frames_diag.rs`, `src/frames_logic.rs`, `src/display_link.rs`, `src/window.rs`, `src/platform.rs` | — |

## Диагностика кадров

Выключена по умолчанию; включается, если `GPUI_FRAMES_WATCHDOG=1` или цель `gpui_macos::frames` открыта на debug
(решается раз за процесс). Выключена — окна не заводят записи, потока сторожа нет.

- Запись: `t=<мс от запуска> w<окно>` (`w0` — приложение). DEBUG — смены состояния, TRACE — каждый кадр и пробуждение.
- Metal-путь: каждый `wake()` с исходом (`resumed`, `link running`, `deferred to end_frame`, `no link (detached)`,
  `link busy`), каждый `end_frame` (рисовал, предъявил, спрос gpui, решение о паузе), `start_display_link` /
  `stop_display_link` с причиной, каждый колбэк ссылки. CVDisplayLink-путь — колбэки, слитые тики, пропуски.
- `displayLayer:`, `viewDidChangeBackingProperties`, `windowDidChangeScreen`, `windowDidChangeOcclusionState`,
  ключевое окно, свёртывание, видимость (`occlusionState`, `isVisible`, `isKeyWindow`, `isMiniaturized`,
  `isOnActiveSpace`, `maximumFramesPerSecond` экрана); активация приложения, сон и пробуждение экранов и системы,
  смена Space; `thermalState` и `isLowPowerModeEnabled`.
- Сторож — поток раз в 250 мс: gpui ждёт кадра (`schedule_frame` или пробуждение после последнего кадра) дольше
  500 мс — WARN `frame stall` со снимком (ссылка, видимость, экран, питание, App Nap, ~64 последних события);
  кадр пришёл — WARN `frame stall ended after N ms` и чем закончилось (`displayLayer:`, колбэк, пробуждение).
  Только пишет, ничего не чинит. «Грязное окно / `on_next_frame`» платформе не видно: спрос — это вызовы
  `schedule_frame` и waker'а, которые gpui делает ровно при этих условиях.
- Drawable Metal-ссылки: `presentedHandler` каждого (время показа, в работе = выдано − показано − отброшено −
  возвращено неиспользованным), `maximumDrawableCount`, `displaySyncEnabled`, `presentsWithTransaction`,
  `allowsNextDrawableTimeout` слоя; кадры без drawable ссылки — время `draw` через `nextDrawable` (nil пишет сам
  gpui_apple: «failed to retrieve next drawable»); интервалы последних колбэков ссылки. Всё это — и в снимке сторожа;
  длительность паузы — до первого кадра после неё. `preferredFrameRateRange` не виден: ссылка закрыта в gpui_apple.
- При первом окне — INFO: версия macOS, `hw.model`, экраны с частотой, выбранный путь кадров.

Прогон с диагностикой: `GPUI_FRAMES_WATCHDOG=1 POSTMAN_LOG=info,gpui_macos::frames=trace`. Пока окно не видно
(перекрыто, другой Space, свёрнуто), спрос без кадра — не пауза: DEBUG `frame demand while hidden`, не WARN.
Чистая логика (кольцо, порог сторожа) — `src/frames_logic.rs`, её тесты идут в `pets-gpui-macos` на любой ОС.

## Вывод GMAC-01 (2026-11-13)

Паузы 7–55 с на Metal-пути — особенность виртуального дисплея macOS-раннеров CI (`VirtualMac2,1`), а не gpui:
- CI: во время паузы ссылка подключена и не на паузе, пробуждения до неё доходят, drawable в работе — 0, а колбэков
  нет; у всех drawable `presentedTime=0` — виртуальный дисплей не показывает ни одного кадра. Возврат — часто на
  границе минуты (системная перерисовка).
- Настоящий Mac (M4, 120 Гц, macOS 26.5, 7 мин): пока окно видно — ни одного пропуска дольше 150 мс; все «паузы» —
  при действительно скрытом окне (другой Space или приложение, перекрытие), возврат к окну — 25–50 мс.

Поэтому путь кадров — стандартный (решение владельца «как положено по стандарту вендора»): Apple рекомендует
`CAMetalDisplayLink` для Metal на macOS 14+, CVDisplayLink устарел с macOS 15. Отчёт в zed не нужен. Снимки Postman на
CI не ждут ссылку: перед кадром просят систему перерисовать окно (`pets_gpui_macos::window::redraw`, `displayLayer:`).

По коду форк теперь почти совпадает с исходником; держим его ради диагностики и отката.

Тест нашей части: `cargo test --manifest-path crates/pets-gpui-macos-platform/Cargo.toml --lib -- pets_` (macOS);
логика сторожа — `cargo test -p pets-gpui-macos` (любая ОС).

При обновлении gpui-kit: взять новый `gpui-pre-macos` с crates.io, перенести правки «Pets patch» и прогнать снимки
Postman (`macos.yml`, вход `frames_diag`: 3 прогона без `frame stall` при видимом окне).
