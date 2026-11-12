# pets-gpui-base

`gpui-base` gpui-kit с нашим патчем. Пакет называется как оригинал — `gpui-base` 0.7.3 (снимок longbridge/gpui-kit
@67265a3, `crates/base`, Apache-2.0; их README — `README.upstream.md`), чтобы подменять его через `[patch]`, как
`pets-gpui-windows`:

```toml
[patch.crates-io]
gpui-base = { git = "https://github.com/Dicobraz/pets", rev = "<коммит>" }
```

Каталог исключён из рабочей области `pets` (чужой код со своим стилем и `Cargo.lock`). Первый коммит каталога — снимок
как есть, правки — следующими коммитами; в коде они помечены «Pets patch».

| Патч | Файлы | Потребитель |
|---|---|---|
| Блоки в тексте поля, как в Zed: `EditorBlock { id, lines, extra_rows, cover }`, `EditorState::set_blocks(blocks, renderer)`, `clear_blocks`. Блок рисует элемент приложения поверх строк `lines` (`cover`) и/или поверх `extra_rows` пустых строк, отведённых после последней из них. Отведённые строки — настоящие строки отображения (кратны высоте строки): прокрутка, полоса прокрутки, попадание мышью и курсор по вертикали остаются в прежней модели одинаковых строк; щелчок по отведённой строке ставит курсор в конец последней строки текста, стрелки перешагивают отведённые строки | `src/input/editor/blocks.rs`, `editor/display_map/{text_wrapper,wrap_map,display_map}.rs`, `base/{element,movement,state}.rs`, `editor/indent.rs`, `src/input/mod.rs` | Postman T3-06/T3-07: таблица письма сеткой поверх своего Markdown, логотип подписи своего размера в тексте |
| HTML-стек html5ever 0.39 + markup5ever_rcdom 0.39.0+unofficial вместо 0.27 + 0.3 (upstream 0.9.1 всё ещё на 0.27): одна копия html5ever/markup5ever/xml5ever/tendril/string_cache/phf с Postman (`postman-ir`). API совместим, код не менялся — только версии | `Cargo.toml`, `Cargo.lock` | Postman: без второй копии HTML-стека в бинаре |

Блоки адресуются строками текста: приложение задаёт их заново после каждой правки. Текст под блоком остаётся текстом
поля (правится, выделяется, копируется); приложение его прячет — подложкой блока или цветом украшения.

Тесты нашей части: `cargo test --manifest-path crates/pets-gpui-base/Cargo.toml --lib --features test-support -- block_rows`.

При обновлении gpui-kit: взять новый `gpui-base` с crates.io, перенести помеченные правки, прогнать тесты и сцены
письма Postman (`compose-t306-*`, `compose-t307-*`). Кандидаты в upstream (longbridge/gpui-kit): блоки — общий механизм
редактора, у Zed он есть; html5ever 0.39 — обновление зависимости без правок кода.
