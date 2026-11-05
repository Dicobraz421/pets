# 2026-10-21. pets-commands: схемы клавиш (Postman S5-17)

Несколько наборов сочетаний у одной таблицы команд — «как Outlook» по умолчанию и «одиночные клавиши как Gmail».
API (добавлено, прежнее не менялось):
- `Scheme { id, keys: &[SchemeKeys] }`, `SchemeKeys { command, keys, context }` — слой сочетаний схемы со своим
  контекстом (одиночные буквы — «вне полей ввода»); клавиши по умолчанию — по-прежнему `Command::keys`.
- `bindings(commands, scheme, same_place) -> Vec<Binding>` — итоговые привязки: по умолчанию, затем слой;
  сочетание по умолчанию, которое слой дал там же (`same_place` решает приложение), уходит. `Binding { command,
  keys, context, from_scheme }`.
- `validate_scheme(commands, scheme)` — команды есть, сочетания разобраны, в слое без повторов в одном контексте;
  `Problem::UnknownCommand`.
- Свои клавиши пользователя — в приложении поверх `bindings`. Потребитель — Postman (Урфин pets-commands пока не
  подключает).
