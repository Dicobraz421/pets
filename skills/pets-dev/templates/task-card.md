# <ID>: <суть одной строкой>

lane: <полоса из lanes.toml>
deps: [<ID>, …]            # закрытые раньше
class: A                    # B — если меняет пути CODEOWNERS
files: [crates/x/src/y.rs, …]  # всё в одной полосе (+ common)
read: [docs/specs/S5.md#S5-12, docs/CONTRACTS.md#Focus]   # только нужные разделы
checks: [screens:<сцены>]   # + release, если перф/размер; clippy/nextest — CI
manual: yes|no              # yes → docs/manual-tests/<ID>.md

## Сделать
- …

## Не делать
- … (смежное, что легко зацепить; замеченное сверх — в «Решения для владельца»)

## Приёмка
- … (проверяемо: имя теста или сцена снимка — это и есть доказательство в PR)
