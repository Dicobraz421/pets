# pets-gpui-windows

Windows-платформа gpui с нашими патчами. Пакет называется как оригинал — `gpui-pre-windows` 0.3.12
(снимок `gpui_windows` из zed@d2a4fb6, Apache-2.0), чтобы подменять его через `[patch]`:

```toml
[patch.crates-io]
gpui-pre-windows = { git = "https://github.com/Dicobraz/pets", rev = "<коммит>" }
```

Каталог исключён из рабочей области `pets` (чужой код со своим стилем и `Cargo.lock`). Наши правки помечены
в коде «Postman patch» / «Postman:» (сделаны в Postman, фаза 0) и «Pets patch» (сделаны здесь):

| Патч | Файлы | Рецепт |
|---|---|---|
| Шейдеры компилируются при сборке через `D3DCompile` в `build.rs` (без fxc.exe, без путей машины сборки) и вшиваются байткодом; при запуске — 0,4 мс вместо ~250 мс. Сборка не на Windows — компиляция при запуске (`cfg(shaders_runtime)`) | `build.rs`, `directx_renderer.rs`, `Cargo.toml` | — |
| Кадры по запросу: поток vsync спит без запросов кадра | `vsync.rs`, `platform.rs`, `window.rs`, `events.rs` | `docs/recipes/frames-on-demand.md` |
| Живой ресайз: кадр на `WM_SIZE` в цикле перетаскивания, swapchain окна с растяжением, DirectComposition выключена по умолчанию (`GPUI_DISABLE_DIRECT_COMPOSITION=0` — включить) | `events.rs`, `window.rs`, `directx_renderer.rs`, `platform.rs` | `docs/recipes/live-resize.md` |
| Фон при ресайзе: `WM_ERASEBKGND` заливает цветом фона прошлого кадра (первый сплошной непрозрачный квад во всё окно — `bg` корня), а не чёрной кистью класса; до первого кадра — чёрная | `events.rs`, `window.rs` | `docs/recipes/live-resize.md` |
| Без чёрного кадра на старте: окно скрыто DWM (`DWMWA_CLOAK`) с создания до первого кадра, который GPU доделал и DWM забрал (запрос-событие и `DwmFlush` после Present; показ сразу после Present давал чёрный кадр на один vsync) — на экран попадает уже кадр приложения, а не чёрная кисть класса (Postman: 80–200 мс чёрного окна → 0, замер Windows Graphics Capture) | `window.rs` | `docs/recipes/live-resize.md` |
| Ленивые текстуры путей: промежуточные текстуры путей (обычная + MSAA 4×, размером с окно) создаются в первом кадре с путями и сбрасываются при ресайзе — окно без путей не держит ~33 МБ (1600×1000), ресайз не копит остатки | `directx_renderer.rs` | `docs/recipes/gpui-pitfalls.md` (Память) |
| Принудительный WARP (программный Direct3D): `GPUI_FORCE_WARP=1` — устройство создаётся на `EnumWarpAdapter`, без перебора GPU; читается раз за процесс, пересоздание после потери устройства — тоже на WARP | `directx_devices.rs` | — |
| Платформенный код окна для приложений Pets за нейтральным API (`gpui_windows::pets`): перетаскивание файлов из окна (OLE drag source), значки окна из ресурса exe, снимок окна, уведомление; `unsafe` — только там (без UI — в `pets-shell`). Зависимость `windows-sys` — для этого модуля | `src/pets/`, `gpui_windows.rs`, `Cargo.toml` | `docs/decisions.md` (2026-10-28) |
| Из gpui-fast (Longbridge, Apache-2.0, `gpui-fast-windows` 0.1.7, `src/fast/`): батчи кадра рисуются через `DrawState` — пайплайн, семплер, текстура и начало батча ставятся, только если изменились; буферы вершин и спрайтов путей живут между кадрами; константный буфер `GlobalParams` пишется, только если изменился; дизеринг градиента от начала его границ (не «шумит» при листании). Под графическим отладчиком (PIX) — старый цикл с метками. Тайлы и композиция gpui-fast не перенесены. Кадр побитно равен старому циклу (тест `pets_fast_frame_matches_upstream_loop`) | `src/pets_fast/`, `directx_renderer.rs`, `shaders.hlsl`, `gpui_windows.rs` | — |
| Быстрый путь `layout_line`: простая строка (ASCII, Latin-1, кириллица, пунктуация) собирается из таблицы глифов по (шрифт, размер), без `IDWriteTextLayout`; лигатуры, контекстные замены и кернинг отсекает разбор GSUB/GPOS/`kern`, остальное — старым путём. Результат побитно равен старому пути (тесты `pets_fast_*`), 5 мкс вместо 40 на строку | `direct_write.rs`, `direct_write/fast.rs`, `direct_write/fast_tests.rs` | `docs/recipes/dwrite-fast-path.md` |

## Перетаскивание из окна и метка «из Интернета» (PAR-07)

`gpui_windows::pets::drag_out`: файл с копией на диске (`DragFile::with_copy`) уходит **только** как `CF_HDROP`, и перед
`DoDragDrop` на копию пишется `Zone.Identifier` (`[ZoneTransfer] ZoneId=3`, как браузеры; `mark_from_internet`).
Метка не записалась (копии нет, том без потоков NTFS) — `drag_out` возвращает ошибку, перетаскивания нет. Без копий —
виртуальные файлы (`FILEDESCRIPTORW`/`FILECONTENTS`) и WARN в журнал: метки на результате не будет, отказывать ли —
решает приложение (Postman отказывает, PAR-04).

Почему так, а не `IAttachmentExecute::Save` для виртуального файла (по документации Microsoft):
- виртуальный файл пишет получатель (Проводник — сам, из `IStream`/`HGLOBAL`), источник не знает ни пути, ни момента
  записи; `IAttachmentExecute::Save` применяет метку к файлу, который сохранило *вызывающее* приложение (`SetLocalPath`), —
  к чужой записи его не приложить;
- копию с `CF_HDROP` Проводник копирует `CopyFile`/`IFileOperation`, а они переносят альтернативные потоки NTFS — метка
  доезжает до места назначения (на томе без ADS, например FAT-флешке, её не будет — так у любого файла с меткой);
- Outlook и другие получатели `CF_HDROP` читают файл по пути — вложение берётся из помеченной копии;
- виртуальные форматы рядом с `CF_HDROP` не отдаются: получатель, предпочитающий их, обошёл бы метку.
Метка пишется напрямую, а не через `IAttachmentExecute`: политика «не сохранять сведения о зоне» и антивирусная проверка
внутри `Save` могли бы снять метку или удалить копию — поведение должно быть одним (ADR Postman 0011).

Тесты нашей части: `cargo test --manifest-path crates/pets-gpui-windows/Cargo.toml --lib --features test-support -- vsync force_warp lazy_path image_sprite background pets_`
(`pets_drag_copy_gets_mark_of_the_web` — поток `Zone.Identifier` на копии, отказ без копии)
(`lazy_path` рисует путь в скрытом окне через `render_to_image` и проверяет пиксели до и после ресайза;
`image_sprite` — кадр 2000×1400 больше страницы атласа и кадр, обрезанный маской: на экране нужные пиксели кадра;
`pets_fast_` — быстрый путь `layout_line` побитно равен старому на корпусе строк, сводка — с `--nocapture`;
кадр через `DrawState` побитно равен старому циклу, дизеринг градиента сдвигается вместе с ним).

При обновлении gpui-kit: взять новый `gpui-pre-windows` с crates.io, перенести помеченные правки, прогнать тесты,
бенч и `resize-check` приложения; быстрый путь — по списку из `docs/recipes/dwrite-fast-path.md`. Кадры по запросу и живой ресайз — кандидаты в zed-industries/zed.
