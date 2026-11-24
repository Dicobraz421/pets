use super::*;

const EN: &str = "\
hello = Hello
only-en = Only in English
files = { $n ->
    [one] { $n } file
   *[other] { $n } files
}
greet = Hello, { $name }!
";

const RU: &str = "\
hello = Привет
files = { $n ->
    [one] { $n } файл
    [few] { $n } файла
   *[many] { $n } файлов
}
greet = Привет, { $name }!
";

/// Набор библиотеки поверх набора приложения.
const LIB: &[Resource] = &[("en", "lib-ok = OK\n"), ("ru", "lib-ok = Хорошо\n")];
const APP: &[Resource] = &[("en", EN), ("ru", RU)];

fn build(lang: &str) -> I18n {
    let (i18n, problems) = I18n::new(&[APP, LIB], lang);
    assert_eq!(problems, []);
    i18n
}

fn n(value: i64) -> FluentArgs<'static> {
    let mut args = FluentArgs::new();
    args.set("n", value);
    args
}

#[test]
fn languages_switch() {
    let ru = build("ru");
    assert_eq!(ru.lang(), "ru");
    assert_eq!(ru.get("hello"), "Привет");
    assert_eq!(ru.get("lib-ok"), "Хорошо");
    let en = build("en");
    assert_eq!(en.lang(), "en");
    assert_eq!(en.get("hello"), "Hello");
    assert_eq!(en.get("lib-ok"), "OK");
    assert_eq!(build("ru-RU").lang(), "ru", "по первичному подтегу");
}

#[test]
fn missing_key_falls_back_to_english_then_key() {
    let ru = build("ru");
    assert_eq!(ru.get("only-en"), "Only in English");
    assert_eq!(ru.get("no-such-key"), "no-such-key");
}

#[test]
fn unknown_language_is_a_problem() {
    let (i18n, problems) = I18n::new(&[APP], "de");
    assert_eq!(i18n.lang(), "en");
    assert_eq!(
        problems,
        [Problem::UnknownLanguage {
            requested: "de".into(),
            used: "en".into()
        }]
    );
    // Без EN — первый язык из наборов.
    let (i18n, _) = I18n::new(&[&[("ru", RU)]], "de");
    assert_eq!(i18n.lang(), "ru");
    // Язык ОС, которого нет, — не ошибка.
    let (_, problems) = I18n::new(&[APP], "auto");
    assert_eq!(problems, []);
}

#[test]
fn russian_plurals_and_no_isolation_marks() {
    let ru = build("ru");
    assert_eq!(ru.fmt("files", &n(1)), "1 файл");
    assert_eq!(ru.fmt("files", &n(3)), "3 файла");
    assert_eq!(ru.fmt("files", &n(5)), "5 файлов");
    let mut args = FluentArgs::new();
    args.set("name", "Аня");
    assert_eq!(ru.fmt("greet", &args), "Привет, Аня!", "без U+2068/U+2069");
    assert_eq!(build("en").fmt("files", &n(1)), "1 file");
}

#[test]
fn new_reports_parse_errors_and_duplicates_but_works() {
    let broken: &[Resource] = &[("en", "ok = Fine\nbad = { $n ->\n"), ("en", "ok = Again\n")];
    let (i18n, problems) = I18n::new(&[broken], "en");
    assert_eq!(i18n.get("ok"), "Fine", "действует первое определение");
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, Problem::Parse { lang, line, .. } if lang == "en" && *line >= 2))
    );
    assert!(problems.contains(&Problem::Duplicate {
        lang: "en".into(),
        key: "ok".into()
    }));
}

#[test]
fn check_finds_missing_duplicate_and_variables() {
    assert_eq!(check(&[LIB]), []);
    let ru = "hello = Привет\nhello = Ещё раз\ngreet = Привет, { $user }!\n";
    let en = "hello = Hello\ngreet = Hello, { $name }!\nonly-en = English\n";
    let problems = check(&[&[("en", en), ("ru", ru)]]);
    assert_eq!(
        problems,
        [
            Problem::Duplicate {
                lang: "ru".into(),
                key: "hello".into()
            },
            Problem::Variables {
                key: "greet".into(),
                lang: "en".into(),
                vars: "$name".into(),
                other: "ru".into(),
                other_vars: "$user".into()
            },
            Problem::Missing {
                key: "only-en".into(),
                present: "en".into(),
                missing: "ru".into()
            },
        ]
    );
}

#[test]
fn check_sees_variables_in_selectors_and_variants() {
    let en = "files = { $n ->\n    [one] one\n   *[other] { $n } in { $dir }\n}\n";
    let ru = "files = { $n ->\n    [one] один\n   *[other] { $n } в { $dir }\n}\n";
    assert_eq!(check(&[&[("en", en), ("ru", ru)]]), []);
    let ru = "files = { $n } файлов\n";
    assert_eq!(check(&[&[("en", en), ("ru", ru)]]).len(), 1);
}

#[test]
fn raw_strings_flags_ui_text() {
    let src = "fn view() {\n    div().child(\"Привет\")\n}\n";
    assert_eq!(
        raw_strings(src),
        [RawString {
            line: 2,
            text: "Привет".into()
        }]
    );
}

#[test]
fn raw_strings_ignores_comments_lifetimes_and_tests() {
    let src = r####"
// Комментарий "Привет"
/// Документация "Привет"
/* блок /* вложенный "Привет" */ всё ещё "Привет" */
fn f<'a>(x: &'a str) -> &'a str { x }
const C: char = '"';
const N: char = '\n';
const Q: char = '\'';
const Я: char = 'Я';
const B: u8 = b'"';
const K: &str = "key \" with escape";
const R: &str = r#"raw "quoted" text"#;
const T: &str = "Привет"; // i18n-ok
let r#type = 1;
'outer: loop { break 'outer; }
#[cfg(test)]
mod tests { const X: &str = "Привет"; }
"####;
    assert_eq!(raw_strings(src), []);
}

#[test]
fn raw_strings_handles_raw_strings() {
    let src = "let a = r#\"Тест \"в кавычках\"\"#;\nlet b = r\"Ещё\";\nlet c = br\"bytes\";\n";
    assert_eq!(
        raw_strings(src),
        [
            RawString {
                line: 1,
                text: "Тест \"в кавычках\"".into()
            },
            RawString {
                line: 2,
                text: "Ещё".into()
            },
        ]
    );
}

#[test]
fn raw_strings_counts_lines_across_multiline_literals() {
    let src = "let a = \"line\nnext\";\n/* one\ntwo */\nlet b = \"Привет\";\n";
    assert_eq!(raw_strings(src).first().map(|r| r.line), Some(5));
}

/// Язык потока не виден другим потокам и не зависит от порядка тестов: глобальное состояние не трогает.
#[test]
fn scoped_language_is_local_to_its_thread() {
    let other = std::thread::spawn(|| {
        let _en = scoped(&[APP, LIB], "en");
        assert_eq!(tr!("hello"), "Hello");
    });
    {
        let _ru = scoped(&[APP, LIB], "ru");
        assert_eq!(current().lang(), "ru");
        assert_eq!(tr!("hello"), "Привет");
        assert_eq!(tr!("lib-ok"), "Хорошо");
    }
    // Значок погашен — снова глобальный язык (каким бы он ни был из других тестов).
    assert_ne!(current().lang(), "ru");
    other.join().unwrap();

    let threads: Vec<_> = (0..16)
        .map(|i| {
            let lang = if i % 2 == 0 { "ru" } else { "en" };
            std::thread::spawn(move || {
                for _ in 0..200 {
                    let _guard = scoped(&[APP, LIB], lang);
                    assert_eq!(current().lang(), lang);
                    assert_eq!(tr!("hello"), if lang == "ru" { "Привет" } else { "Hello" });
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
}

#[test]
fn scoped_language_nests_and_the_last_wins() {
    let _ru = scoped(&[APP, LIB], "ru");
    {
        let _en = scoped(&[APP, LIB], "en");
        assert_eq!(tr!("hello"), "Hello");
    }
    assert_eq!(tr!("hello"), "Привет");
}

#[test]
fn scoped_language_reports_problems_without_stopping() {
    let _lang = scoped(&[APP, LIB], "no-such-language");
    assert_eq!(current().lang(), "en", "незнакомый язык — запасной");
    assert_eq!(tr!("hello"), "Hello");
}

/// Единственный тест на глобальное состояние: остальные не трогают его, чтобы не мешать друг другу.
#[test]
fn global_language_switches_on_the_fly() {
    assert_eq!(init(&[APP, LIB], "ru"), []);
    assert_eq!(current().lang(), "ru");
    assert_eq!(tr!("hello"), "Привет");
    assert_eq!(tr!("files", n = 3), "3 файла");
    assert_eq!(tr!("greet", name = "Аня"), "Привет, Аня!");
    let before = epoch();
    assert_eq!(set_language("en"), []);
    assert!(epoch() > before);
    assert_eq!(tr!("hello"), "Hello");
    assert_eq!(tr!("lib-ok"), "OK");
}
