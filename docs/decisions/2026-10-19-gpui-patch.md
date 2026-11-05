# 2026-10-19. Патч gpui в pets

Патч Windows-платформы gpui из Postman (`third_party/gpui-pre-windows`) — в `crates/pets-gpui-windows`.
Имя пакета прежнее (`gpui-pre-windows`, Apache-2.0, снимок Zed) — для `[patch]`; каталог вне рабочей
области `pets`, со своим `[workspace]` и `Cargo.lock`; CI — отдельная задача с тестами наших правок.
