//! Linux: общее для `url_scheme` и `login_item` — каталоги XDG, поиск `.desktop` этого исполняемого файла, строка
//! `Exec` (спецификация Desktop Entry 1.5), вызов утилит `xdg-utils`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ShellError;

/// Каталог из переменной `var` (только абсолютный путь), иначе `$HOME/<fallback>`.
fn xdg_home(var: &str, fallback: &str) -> Option<PathBuf> {
    match std::env::var_os(var).map(PathBuf::from) {
        Some(d) if d.is_absolute() => Some(d),
        _ => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)),
    }
}

/// `$XDG_CONFIG_HOME` (по умолчанию `~/.config`).
pub(crate) fn config_home() -> Option<PathBuf> {
    xdg_home("XDG_CONFIG_HOME", ".config")
}

/// Каталоги `applications` по порядку приоритета: `$XDG_DATA_HOME`, затем `$XDG_DATA_DIRS`.
fn application_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = xdg_home("XDG_DATA_HOME", ".local/share").into_iter().collect();
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').map(PathBuf::from).filter(|d| d.is_absolute()));
    dirs.into_iter().map(|d| d.join("applications")).collect()
}

/// Значение ключа `key` группы `[Desktop Entry]` (без локализации и без раскрытия экранирования).
pub(crate) fn entry_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry
            && let Some((k, v)) = line.split_once('=')
            && k.trim_end() == key
        {
            return Some(v.trim_start());
        }
    }
    None
}

/// Раскрыть экранирование строкового значения (`\s`, `\n`, `\t`, `\r`, `\\`).
fn unescape_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

/// Первое слово `Exec` — программа (кавычки и экранирование по спецификации).
pub(crate) fn exec_program(exec: &str) -> Option<String> {
    let exec = unescape_value(exec);
    let mut chars = exec.trim_start().chars();
    let mut program = String::new();
    match chars.next()? {
        '"' => loop {
            match chars.next()? {
                '"' => break,
                '\\' => program.push(chars.next()?),
                c => program.push(c),
            }
        },
        first => {
            program.push(first);
            program.extend(chars.take_while(|c| !c.is_whitespace()));
        }
    }
    (!program.is_empty()).then_some(program)
}

/// Значение `Exec` для программы `exe`: путь в кавычках (пробелы, кириллица), `extra` — коды полей (`%u`).
pub(crate) fn exec_line(exe: &Path, extra: &str) -> String {
    let mut quoted = String::from("\"");
    for c in exe.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    // Значение ключа — строка: обратная косая черта удваивается ещё раз.
    let mut line = quoted.replace('\\', "\\\\");
    if !extra.is_empty() {
        line.push(' ');
        line.push_str(extra);
    }
    line
}

/// Программа `Exec` указывает на `exe`: тот же файл после разрешения ссылок, имя без `/` — поиск в `$PATH`.
fn same_program(program: &str, exe: &Path) -> bool {
    let candidate = if program.contains('/') {
        Some(PathBuf::from(program))
    } else {
        std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(program)).find(|c| c.is_file()))
    };
    candidate
        .and_then(|c| std::fs::canonicalize(c).ok())
        .is_some_and(|c| c == exe)
}

/// Идентификатор `.desktop` этого исполняемого файла (`postman.desktop`; из подкаталога `a/b.desktop` — `a-b.desktop`):
/// первая запись каталогов `applications`, у которой программа `Exec` — этот процесс. Нет — `None`.
pub(crate) fn desktop_id() -> Option<String> {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|e| std::fs::canonicalize(e).ok())?;
    application_dirs().iter().find_map(|dir| find_in(dir, dir, &exe, 2))
}

fn find_in(root: &Path, dir: &Path, exe: &Path, depth: u8) -> Option<String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if depth > 0
                && let Some(id) = find_in(root, &path, exe, depth - 1)
            {
                return Some(id);
            }
            continue;
        }
        if path.extension().is_none_or(|e| e != "desktop") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let matches = entry_value(&text, "Exec")
            .and_then(exec_program)
            .is_some_and(|p| same_program(&p, exe));
        if matches {
            let rel = path.strip_prefix(root).ok()?;
            return Some(rel.to_string_lossy().replace('/', "-"));
        }
    }
    None
}

/// Запустить утилиту `program` с `args`; вывод — строка stdout без перевода строки в конце.
pub(crate) fn run(program: &'static str, args: &[&str]) -> Result<String, ShellError> {
    let out = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|source| ShellError::Os { what: program, source })?;
    if !out.status.success() {
        return Err(ShellError::Os {
            what: program,
            source: std::io::Error::other(format!("exit status {}", out.status)),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// `Exec` в кавычках с пробелом и `%` разбирается обратно в тот же путь.
    #[test]
    fn exec_line_round_trip() {
        let exe = Path::new("/opt/Мой Post$man/100%/postman");
        let line = super::exec_line(exe, "%u");
        assert!(line.ends_with(" %u"));
        // `%%` в `Exec` — буквальный `%` (раскрывает запускающий; здесь сравниваем до раскрытия).
        let program = super::exec_program(&line).unwrap().replace("%%", "%");
        assert_eq!(program, exe.to_str().unwrap());
        assert_eq!(super::exec_program("postman %u").as_deref(), Some("postman"));
    }

    /// Ключ берётся только из `[Desktop Entry]`.
    #[test]
    fn entry_value_reads_main_group() {
        let text = "[Desktop Action new]\nExec=other\n[Desktop Entry]\nName=Postman\nExec = /usr/bin/postman %u\n";
        assert_eq!(super::entry_value(text, "Exec"), Some("/usr/bin/postman %u"));
        assert_eq!(super::entry_value(text, "Icon"), None);
    }
}
