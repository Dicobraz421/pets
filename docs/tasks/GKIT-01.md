# GKIT-01: pets на gpui-kit 0.9.1 (gpui-pre 0.3.12)

lane: x (gpui-windows + deps + docs; класс B согласован владельцем в чате 2026-11-12)
status: done
deps: []
class: B
files: [crates/pets-gpui-base/, crates/pets-gpui-windows/, Cargo.toml, Cargo.lock, docs/decisions.md, docs/recipes/gpui-pitfalls.md, docs/manual-tests/GKIT-01.md]
read: [crates/pets-gpui-windows/README.md, crates/pets-gpui-base/README.md]
checks: [check/clippy linux, x86_64-pc-windows-msvc, aarch64-apple-darwin (без pets-ui), тесты gpui-base]
manual: yes

> Владелец 2026-11-12: Pets/Postman с gpui-kit 0.7.3 (gpui-pre =0.3.9) на 0.9.1 (gpui-pre =0.3.12).
> Сторона Postman — GKIT-02.

## Сделать
- `pets-gpui-base` → снимок `gpui-base` 0.9.1 (v0.9.1 @39ab849) + патчи из README: блоки в поле, html5ever 0.39 (#71).
- `pets-gpui-windows` → `gpui-pre-windows` 0.3.12 (zed@d2a4fb6) + все патчи таблицы README.
- Рабочая область: `gpui-kit = "=0.9.1"`; остальные крейты собираются без правок кода.

## Приёмка
- `cargo test --manifest-path crates/pets-gpui-base/Cargo.toml --lib --features test-support` — 1633 зелёных, `block_rows` среди них.
- `cargo clippy --target x86_64-pc-windows-msvc` для `pets-gpui-windows` (lib + tests) и рабочей области — без предупреждений.
- Windows-кадры — `docs/manual-tests/GKIT-01.md` (стенд).
