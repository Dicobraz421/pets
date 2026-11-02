# Разовые настройки GitHub (владелец, на каждый репо)

Settings → General → Pull Requests:
- Allow auto-merge — вкл.; Allow merge commits — вкл. (метод очереди).; Automatically delete head branches — вкл.

Settings → Rules → Rulesets → New branch ruleset «main» (target: default branch):
- Restrict deletions, Block force pushes.
- Require a pull request before merging: required approvals 0, **Require review from Code Owners** — вкл.
  (класс B — с approve владельца через CODEOWNERS, класс A — без).
- Require status checks to pass: **`gate`** (только он).
- Require merge queue: method **Merge**, build concurrency 4, min group 1, max group 4,
  wait time 2 min, status check timeout 30 min, «Only merge non-failing pull requests» — вкл.
- Bypass list: пусто (владелец мержит так же, через очередь).

Проверка: docs-PR проходит `gate` без сборки; PR с Cargo.toml ждёт approve владельца.
