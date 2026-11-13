# PAR-07: перетаскивание вложения из окна — всегда с меткой «из Интернета»

lane: x (gpui-windows + docs; класс B согласован владельцем в чате 2026-10-26)
status: done
deps: []
class: B
files: [crates/pets-gpui-windows/src/pets/drag.rs, crates/pets-gpui-windows/README.md, docs/decisions.md, docs/manual-tests/PAR-07.md]
read: [crates/pets-gpui-windows/README.md]
checks: [fmt, clippy x86_64-pc-windows-msvc, тест pets_drag_ в Windows CI]
manual: yes

> Аудит Postman №11 (`docs/tasks/PAR-AUDIT.md`): виртуальный файл пишет Проводник без `Zone.Identifier`.

## Сделать
- Копия есть у всех файлов → только `CF_HDROP`, на копию — `Zone.Identifier` `ZoneId=3`; не вышло — ошибка `drag_out`.
- Копий нет → виртуальные файлы + WARN.
- Обоснование (`IAttachmentExecute` против копии) — README крейта.

## Приёмка
- Тест `pets_drag_copy_gets_mark_of_the_web` (Windows CI), ручная проверка `docs/manual-tests/PAR-07.md`.
