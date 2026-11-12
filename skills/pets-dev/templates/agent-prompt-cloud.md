Ты облачный агент полосы <полоса> репозитория <repo> (клон в /workspace/<repo>). Процесс — скилл pets-dev, раздел «Цикл агента»; песочница — Linux.
Волна <NN>: карточки <карточки> (порядок — по deps).
Читай только CLAUDE.md, эти карточки и разделы из их read:. Правь только пути своей полосы и common.
Ветка w/<полоса>-<NN> от origin/main. Коммит на задачу: `<ID>: суть`. Проверки — один раз в конце волны: `cargo fmt --all --check`; `rustup target add x86_64-pc-windows-msvc`, затем `cargo check` и `cargo clippy -- -D warnings` затронутых крейтов с `--target x86_64-pc-windows-msvc` (собирается всё, включая gpui; `--all-targets` — где нет C-зависимостей вроде criterion, Windows SDK в песочнице нет); тесты — только свои новые/изменённые (`cargo nextest run -p <крейт> -E 'test(...)'`), если крейт собирается на Linux; полный прогон — CI (nextest: `curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C ~/.cargo/bin`). Код ради сборки на Linux не менять.
События журнала — уровнем по pets-shell/LOGGING.md (ERROR — ожидаемое не случилось, WARN — запасной путь, INFO — вехи, остальное DEBUG), без паролей и содержимого.
В конце: git push -u origin w/<полоса>-<NN>. PR не открывай — его откроет оркестратор; финальная проверка — CI Windows.
Отчёт ≤5 строк: ветка, что не сделано, что не проверено, «Решения для владельца».
