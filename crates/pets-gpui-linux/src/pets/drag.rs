//! Перетаскивание файлов из окна наружу (вложение письма → файловый менеджер).
//!
//! В отличие от OLE на Windows сеанс не модальный и идёт через сам gpui: элемент с `on_drag` объявляет
//! внешнее содержимое `external_drag_payload`, и когда указатель с зажатой кнопкой уходит за окно, gpui отдаёт
//! перетаскивание платформе (`PlatformWindow::start_external_drag`): X11 — XDND, Wayland — `wl_data_device`.
//! Получатель (Nautilus, Dolphin, Thunar…) читает `text/uri-list` и копирует файлы сам, поэтому файлы лежат на
//! диске, пока он их не скопирует, — копии во временном каталоге приложения ([`TempCopies`]). Бросок обратно в
//! своё окно на X11 — отмена.
//!
//! ```ignore
//! div().id("attachment")
//!     .on_drag(Attachment(id), |_, _, _, cx| cx.new(|_| Ghost))
//!     .external_drag_payload::<Attachment>(|a, _, cx| Some(drag::payload(&copies_of(a, cx)?)))
//! ```

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{ExternalDragPayload, FileDragPaths};

/// Порог начала перетаскивания в точках окна (у X11 и Wayland своего числа нет; так же ведут себя GTK и Qt).
pub fn threshold(_scale: f32) -> (f32, f32) {
    (4., 4.)
}

/// Содержимое внешнего перетаскивания: файлы `paths` (копии на диске).
pub fn payload(paths: &[PathBuf]) -> ExternalDragPayload {
    ExternalDragPayload::Files(FileDragPaths::new(
        paths.iter().map(|path| (path.clone(), path.is_dir())),
    ))
}

/// Каталог копий для перетаскивания: `$TMPDIR/<app>/drag`, свой подкаталог на каждое перетаскивание — имена файлов
/// разных источников не сталкиваются. Чистить при старте (остатки прошлого запуска) и при выходе.
#[derive(Debug, Clone)]
pub struct TempCopies {
    root: PathBuf,
}

impl TempCopies {
    /// `$TMPDIR/<app>/drag`; `app` — техническое имя приложения (`Postman`).
    pub fn new(app: &str) -> Self {
        Self::at(std::env::temp_dir().join(app).join("drag"))
    }

    /// Свой корень (тесты, portable).
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Новый пустой подкаталог под копии одного перетаскивания.
    pub fn slot(&self) -> io::Result<PathBuf> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = self
            .root
            .join(format!("{nanos:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Удалить все копии; каталога нет — не ошибка.
    pub fn clean(&self) -> io::Result<()> {
        match std::fs::remove_dir_all(&self.root) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::ExternalDragPayload;

    /// Копии — в своих подкаталогах; чистка убирает всё, повторная — не ошибка.
    #[test]
    fn pets_temp_copies_are_cleaned() {
        let root = std::env::temp_dir().join(format!("pets-drag-test-{}", std::process::id()));
        let copies = super::TempCopies::at(root.clone());
        let (a, b) = (copies.slot().unwrap(), copies.slot().unwrap());
        assert!(a != b && a.starts_with(&root) && b.is_dir());
        copies.clean().unwrap();
        assert!(!root.exists());
        copies.clean().unwrap();
    }

    /// Содержимое — пути с признаком каталога; `text/uri-list` X11 — `file://` через CRLF, с экранированием.
    #[test]
    fn pets_payload_and_uri_list() {
        let dir = std::env::temp_dir();
        let file = dir.join("письмо 1.eml");
        let payload = super::payload(&[dir.clone(), file.clone()]);
        let ExternalDragPayload::Files(paths) = &payload;
        assert_eq!(paths.entries(), &[(dir, true), (file, false)]);
        #[cfg(feature = "x11")]
        {
            let list = String::from_utf8(crate::linux::pets_uri_list(&payload)).unwrap();
            let lines: Vec<_> = list.split_terminator("\r\n").collect();
            assert_eq!(lines.len(), 2);
            assert!(
                lines[1].starts_with("file:///") && lines[1].ends_with("%D0%BF%D0%B8%D1%81%D1%8C%D0%BC%D0%BE%201.eml")
            );
        }
    }
}
