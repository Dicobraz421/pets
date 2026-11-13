# pets-gpui-linux

Linux-платформа gpui с нашими патчами. Пакет называется как оригинал — `gpui-pre-linux` 0.3.12 (снимок
`gpui_linux` из zed@d2a4fb6 с crates.io, Apache-2.0), чтобы подменять его через `[patch]`:

```toml
[patch.crates-io]
gpui-pre-linux = { git = "https://github.com/Dicobraz/pets", rev = "<коммит>" }
gpui-pre-wgpu = { git = "https://github.com/Dicobraz/pets", rev = "<коммит>" }
```

Каталог исключён из рабочей области `pets` (чужой код со своим стилем и `Cargo.lock`). Наши правки помечены
в коде «Pets patch». Эмодзи — в соседнем `crates/pets-gpui-wgpu` (текстовая система Linux живёт там).

| Патч | Файлы | Upstream |
|---|---|---|
| Перетаскивание файлов из окна на X11 — источник XDND: `X11Window::start_external_drag` (upstream — только Wayland), владелец `XdndSelection` — окно-источник, `text/uri-list`, `XdndActionCopy`; цели — по `XdndAware` под указателем (тип свойства любой: gpui сам ставит его с типом-атомом «XA_ATOM»), позиция — с ожиданием `XdndStatus`, Esc — отмена, конец сеанса — `FileDropEvent::Ended` окну. Свои окна целью не считаются | `src/linux/x11/pets_xdnd.rs`, `x11/client.rs`, `x11/window.rs`, `x11.rs` | кандидат |
| Нейтральный API для приложений Pets (`gpui_linux::pets`): `drag::payload`, `TempCopies`, `threshold` — как у `gpui_windows::pets` и `pets_gpui_macos` | `src/pets/`, `gpui_linux.rs` | — |

Перетаскивание наружу в приложении: `div().on_drag(v, …).external_drag_payload::<V>(|v, _, _| Some(drag::payload(&paths)))`
— gpui сам отдаёт перетаскивание платформе, когда указатель уходит за окно (`src/pets/drag.rs`). Wayland —
upstream `wl_data_device` (`wayland/client.rs`, `start_external_drag`).

Тесты нашей части: `cargo test --manifest-path crates/pets-gpui-linux/Cargo.toml --lib --features test-support -- pets_`; проверка под Xvfb —
`docs/manual-tests/GLIN-01.md`.

При обновлении gpui-kit: взять новый `gpui-pre-linux` с crates.io, перенести помеченные правки, прогнать тесты и
`check/dnd.sh` из `docs/manual-tests/GLIN-01`.
