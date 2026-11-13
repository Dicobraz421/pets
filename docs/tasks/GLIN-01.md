# GLIN-01: gpui на Linux — эмодзи и перетаскивание файлов из окна (паритет Postman)

lane: x (gpui-linux, gpui-wgpu)
status: done
deps: []
class: B                    # Cargo.toml (exclude, [patch]), lanes.toml, docs/decisions.md — согласовано владельцем 2026-10-26
files: [crates/pets-gpui-linux/, crates/pets-gpui-wgpu/, Cargo.toml, Cargo.lock, lanes.toml, docs/decisions.md]
read: [Postman docs/decisions/2026-11-12-linux.md, crates/pets-gpui-windows/README.md]
checks: [cargo test --manifest-path crates/pets-gpui-linux/Cargo.toml --lib --features test-support -- pets_, cargo test --manifest-path crates/pets-gpui-wgpu/Cargo.toml --test pets_emoji]
manual: yes                 # docs/manual-tests/GLIN-01.md

> Владелец 2026-11-12: Postman на Linux — паритет с Windows. Две дыры в `gpui-pre-linux` 0.3.12.

## Сделать
- Эмодзи на Linux рисуются цветными (Noto Color Emoji установлен).
- Перетаскивание файла из окна наружу (X11 XDND, Wayland `wl_data_device`, `text/uri-list`) за нейтральным API.

## Итог
- Без форка нельзя: 0.3.12 — последняя `gpui-pre-*`, в zed main то же. Шрифт эмодзи запасным в `Font` не задать —
  `load_family` отбрасывает начертания без `m`.
- Эмодзи: причина — в `gpui-pre-wgpu` (текстовая система Linux, cosmic-text), не в `gpui-pre-linux` и не в атласе:
  общий список замен cosmic-text ставит «Noto Color Emoji» последним, после DejaVu Sans и Noto Sans Symbols2.
  Форк `crates/pets-gpui-wgpu`: графемы с эмодзи-представлением — сразу цветным шрифтом.
- Перетаскивание: upstream уже даёт API (`on_drag` + `external_drag_payload` → `start_external_drag`), Wayland
  реализован, X11 — нет. Форк `crates/pets-gpui-linux`: источник XDND; `gpui_linux::pets::drag` — `payload`,
  `TempCopies`, `threshold`.
- Upstream-кандидаты: оба патча.
