# FAST-0: из gpui-fast-windows — независимые от ядра улучшения рендера

lane: gpui-windows
status: todo
deps: []
class: A
files: [crates/pets-gpui-windows/src/directx_renderer.rs, crates/pets-gpui-windows/src/shaders.hlsl, crates/pets-gpui-windows/src/pets_fast/, crates/pets-gpui-windows/src/gpui_windows.rs, crates/pets-gpui-windows/README.md]
read: [crates/pets-gpui-windows/README.md, docs/recipes/bench.md]
checks: [release, bench]
manual: no

> Решение владельца 2026-11-08: из `gpui-fast-windows 0.1.7` (Longbridge, Apache-2.0, crates.io) взять три вещи,
> не зависящие от ядра gpui-fast. Полезны при любом исходе опыта FAST (Postman `docs/tasks/FAST-1.md`).

## Сделать
- `fast/frame.rs` + `fast/draw_state.rs` → `src/pets_fast/`: батчи рисуются через `DrawState` (без повторных
  bind пайплайна/семплера/текстуры), буферы вершин и спрайтов путей живут между кадрами. Учесть наши ленивые
  текстуры путей (создаются в первом кадре с путями, сбрасываются при ресайзе).
- `fast/globals.rs`: константный буфер `GlobalParams` пишется, только если изменился.
- `shaders.hlsl`: дизеринг градиента — `seed = (position - bounds.origin) * …` (градиент не «шумит» при листании).
- Шапка заимствованного: «Adapted from gpui-fast (Longbridge), Apache-2.0»; метка «Pets patch»; строка в таблице
  README (патч, файлы, откуда).

## Приёмка
- Тесты нашей части из README зелёные; `render_to_image` до и после — побитно равно на сценах с путями и градиентом
  (кроме дизеринга — там ожидаемо иное).
- Бенч Postman (`--demo 50000 --bench`, release, ≥4 чередующихся прогона, медианы) — CPU при листании и работа
  UI-потока за кадр не хуже текущего; цифры — в теле PR.
- Обновление `rev` патча в Postman — отдельная карточка полосы `deps` Postman.
