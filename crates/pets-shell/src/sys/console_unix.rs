//! macOS и Linux: поток ошибок, который ведёт в никуда, — в файл журнала. Правило одно на всех ОС (PAR-03): stderr
//! закрыт или null-устройство (`/dev/null`; Windows — нет описателя или `NUL`) — файл; терминал, файл, канал, сокет
//! (systemd-journald — уже журнал) — не трогаем. macOS: Finder и Dock (launchd отдаёт `/dev/null`).

use std::os::fd::IntoRawFd as _;
use std::path::Path;

use crate::ShellError;

/// stderr ведёт в никуда: закрыт или это `/dev/null`.
fn stderr_is_void() -> bool {
    // SAFETY: `stat` — локальные структуры нужного размера; путь — строка с нулём в конце.
    unsafe {
        let mut err: libc::stat = std::mem::zeroed();
        if libc::fstat(libc::STDERR_FILENO, &mut err) != 0 {
            return true;
        }
        let mut null: libc::stat = std::mem::zeroed();
        libc::stat(c"/dev/null".as_ptr(), &mut null) == 0 && err.st_dev == null.st_dev && err.st_ino == null.st_ino
    }
}

/// Поток ошибок ведёт в никуда (закрыт или `/dev/null`: Finder, Dock, `.desktop`): он — в файл `path` (дозапись), туда
/// пишет `tracing`. stderr — терминал, файл, канал или сокет — ничего. `Ok(true)` — журнал пишется в файл.
pub fn stderr_to_file(path: &Path) -> Result<bool, ShellError> {
    if !stderr_is_void() {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| ShellError::Os {
            what: "log directory",
            source,
        })?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| ShellError::Os {
            what: "log file",
            source,
        })?;
    let fd = file.into_raw_fd();
    // SAFETY: `fd` открыт и живёт до конца процесса (не закрывается); `dup2` заменяет описатель 2 атомарно.
    if unsafe { libc::dup2(fd, libc::STDERR_FILENO) } < 0 {
        return Err(ShellError::Os {
            what: "dup2",
            source: std::io::Error::last_os_error(),
        });
    }
    Ok(true)
}
