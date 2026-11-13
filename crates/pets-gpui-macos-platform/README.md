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
| Кадры — от CVDisplayLink, как в 0.3.9 (gpui-kit 0.7.3); `CAMetalDisplayLink` (macOS 14+) — только по `GPUI_METAL_DISPLAY_LINK=1` (раз за процесс). Окно на 0.3.12 переставало получать кадры на 7–44 с при спросе на них (Postman, снимки macOS CI: 0.7.3 — 17/17 без пауз, 0.9.1 — 4/6 с паузами) | `src/window.rs`, `src/gpui_macos.rs` | кандидат (отчёт ниже) |
| Диагностика кадров (DEBUG, цель `gpui_macos::frames`): пауза Metal-ссылки, отсоединение, пропуск старта у невидимого окна, пробуждение без живой ссылки | `src/display_link.rs`, `src/window.rs` | — |

## Почему CVDisplayLink, а не правка Metal-пути

Доказанной первопричины в Metal-пути пока нет: зависание воспроизводится только на macOS-раннерах Postman, а здесь
(облако Linux) его не воспроизвести. Что видно по коду 0.3.12:
- Metal-ссылка — единственный источник кадров, который сам останавливается при спросе: `MetalFrames::end_frame`
  ставит паузу после обновления, где gpui не рисовал и не будил; снять её может только `wake()` (через waker
  инвалидатора, `schedule_frame`, `on_next_frame`). Waker инвалидатора зовётся лишь при переходе `dirty`
  false→true: если пауза случится при уже грязном окне, следующее пробуждение не придёт.
- `wake()` молча ничего не делает, если ссылки нет (`stop_display_link` её отсоединяет при перекрытии,
  `display_layer`, активации) или она занята (`try_borrow`), а `start_display_link` молча выходит у окна,
  которое AppKit не считает видимым. Дальше кадры приходят только с системной перерисовкой (`display_layer`) —
  это совпадает с паузами «до hh:mm:00».
- CVDisplayLink 0.3.12 — тот же путь, что в 0.3.9: тикает непрерывно, пока окно видимо, пауз по спросу нет.

Возврат CVDisplayLink — откат к проверенному поведению (17/17), а не маскировка: таймеров, тайм-аутов и лишних
перерисовок нет. Metal-путь остаётся за переключателем, чтобы найти точную причину и отправить исправление:
снимки Postman с `GPUI_METAL_DISPLAY_LINK=1 RUST_LOG=gpui_macos::frames=debug` покажут, какое событие предшествует
паузе (пауза при грязном окне, отсоединение без нового старта или потерянное пробуждение).

## Текст отчёта upstream (zed-industries/zed, кандидат)

> **macOS: window stops receiving frames for seconds with `CAMetalDisplayLink` (gpui_macos, zed@d2a4fb6)**
>
> On macOS 14+ a window driven by `FrameSource::Metal` sometimes gets no frame requests for 7–44 s although GPUI
> keeps asking (`request_animation_frame` + `on_next_frame` every frame); frames resume only on the next
> `displayLayer:` (often on a minute boundary). With the `CVDisplayLink` source the same app never stalls (17/17
> CI runs vs 4/6 stalled). Candidates: the link is paused in `MetalFrames::end_frame` while the invalidator is
> already dirty (its waker fires only on a clean→dirty transition); `wake()` is dropped when the link is detached
> or borrowed; `start_display_link` returns early for a window AppKit reports as occluded. Logs with the
> `gpui_macos::frames` debug target attached.

Тест нашей части: `cargo test --manifest-path crates/pets-gpui-macos-platform/Cargo.toml --lib -- pets_` (macOS).

При обновлении gpui-kit: взять новый `gpui-pre-macos` с crates.io, проверить, исправлена ли пауза Metal-ссылки;
если нет — перенести правку и прогнать снимки Postman (`macos.yml`, `bundle --screens`, 3 прогона без пауз).
