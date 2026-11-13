# pets-gpui-wgpu

wgpu-отрисовка и текстовая система Linux (cosmic-text) для gpui с нашими патчами. Пакет называется как оригинал —
`gpui-pre-wgpu` 0.3.12 (снимок `gpui_wgpu` из zed@d2a4fb6 с crates.io, Apache-2.0), подменяется через `[patch]`
вместе с `crates/pets-gpui-linux` (README там). На Windows и macOS не собирается — их платформы его не используют.

| Патч | Файлы | Upstream |
|---|---|---|
| Эмодзи — цветным шрифтом: графема с эмодзи-представлением (UTS #51: `Emoji_Presentation`, `FE0F`, keycap, флаг, ZWJ-последовательность) сразу получает отрезок известного цветного шрифта (Noto Color Emoji, Twemoji, JoyPixels…), если он есть и покрывает её. Было: cosmic-text берёт замену из общего списка Unix, где Noto Color Emoji — последним, после DejaVu Sans и Noto Sans Symbols2, — эмодзи выходили одноцветными контурами или не рисовались; задать шрифт эмодзи запасным в `Font` нельзя — `load_family` отбрасывает начертания без `m` | `src/pets_emoji.rs`, `src/cosmic_text_system.rs`, `src/gpui_wgpu.rs` | кандидат |

Тесты нашей части: `cargo test --manifest-path crates/pets-gpui-wgpu/Cargo.toml --test pets_emoji`; снимок до/после —
`docs/manual-tests/GLIN-01.md`.
