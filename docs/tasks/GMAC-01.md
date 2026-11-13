# GMAC-01: macOS — окно не получает кадры на 7–44 с (регрессия gpui-kit 0.9.1)

lane: gpui-macos
status: done
deps: []
class: B                    # Cargo.toml (exclude, [patch]), Cargo.lock, lanes.toml, docs/decisions.md — согласовано 2026-10-26
files: [crates/pets-gpui-macos-platform/, Cargo.toml, Cargo.lock, lanes.toml, docs/decisions.md]
read: [crates/pets-gpui-windows/README.md]
checks: [macOS CI pets (pets-ui собирается с форком), Postman macos.yml bundle --screens ×3 без пауз]
manual: no

> Блокер беты v0.1.0-beta.6. Postman: снимки macOS — 0.7.3 17/17 без пауз, 0.9.1 4/6 с паузами 7–44 с (часто до
> hh:mm:00 — системной перерисовки); драйвер снимков просит кадр каждый кадр.

## Итог
- Upstream: `gpui-pre-macos` 0.3.12 — последняя; в zed main исправления нет (добавлен только `debug_state`).
- Причина: 0.3.12 на macOS 14+ берёт кадры от `CAMetalDisplayLink` (`FrameSource::Metal`): пауза после кадра без
  отрисовки и запроса, отсоединение при перекрытии, пробуждение через `try_borrow`. 0.3.9 — CVDisplayLink, без пауз.
- Форк `crates/pets-gpui-macos-platform` (пакет `gpui-pre-macos`): CVDisplayLink по умолчанию (откат к проверенному),
  диагностика Metal-пути (DEBUG `gpui_macos::frames`); Metal-ссылка — по `GPUI_METAL_DISPLAY_LINK=1`. Проверить
  зависание здесь нельзя (облако Linux) — проверка в CI macOS и снимках Postman.
- Первопричина в Metal-пути не доказана (нет macOS): кандидаты и текст upstream-отчёта — README форка; доказательство —
  прогон Postman с `GPUI_METAL_DISPLAY_LINK=1 RUST_LOG=gpui_macos::frames=debug`.
