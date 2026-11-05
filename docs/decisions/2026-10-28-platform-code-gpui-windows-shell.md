# 2026-10-28. Платформенный код приложений — в pets-gpui-windows и pets-shell (Postman, T1-18)

Решение владельца: «всё платформозависимое уноси в выделенные модули»: окно — `pets-gpui-windows`, без UI —
`pets-shell`. Приложение не держит `windows`/`windows-sys` и `unsafe` для окна и оболочки ОС. API добавлено, прежнее
не менялось. `gpui_windows::pets` (Pets patch; ошибки — `std::io::Error`):
- `drag`: `DragFile::bytes(name, Arc<Vec<u8>>)` / `DragFile::lazy(name, size, provider)` / `.with_copy(path)`,
  `drag_out(files) -> Outcome { Dropped, Cancelled }` (OLE: виртуальные файлы + запасной `CF_HDROP`), `threshold(scale)`,
  `TempCopies::new(app)` (`%TEMP%\<app>\drag`: `slot`, `clean`). Что тащить и как писать копию — решает приложение.
- `window`: `id(&Window)`, `set_icons(&Window, resource)`, `icon_sizes(resource)`, `capture(id) -> Image`.
- `toast::show(&Sender { aumid, name, icon }, title, body)` — без AUMID процесса (группировка на панели задач прежняя).
`pets_shell` (только Windows, `unsafe` — в `sys`; ошибки — `ShellError`, новый вариант `Invalid`):
- `registry` (HKCU запись, HKCU/HKLM чтение), `credentials` (диспетчер учётных данных), `console` (`attach_parent`,
  `stderr_to_file`, `active_code_page`), `memory::report` (бенч).
