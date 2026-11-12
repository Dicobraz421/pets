//! Перетаскивание файлов из окна в Finder и другие приложения: `-[NSView beginDraggingSessionWithItems:event:source:]`
//! с адресами файлов (`NSURL` — запись в буфер перетаскивания). В отличие от OLE на Windows сеанс не модальный:
//! вызов возвращается сразу, файл копирует получатель, пока его копия лежит во временном каталоге ([`TempCopies`]).
//!
//! Начать сеанс можно только во время события мыши (нажатие или движение с нажатой кнопкой): иначе AppKit бросает
//! исключение, поэтому тип текущего события проверяется до вызова.

use std::cell::OnceCell;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread as _, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource,
    NSEventType, NSWorkspace,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString, NSURL};

define_class!(
    // SAFETY: у `NSObject` нет требований к наследникам; `Drop` нет.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PetsDragSource"]
    struct Source;

    unsafe impl NSObjectProtocol for Source {}

    unsafe impl NSDraggingSource for Source {
        /// Только копия: файл остаётся у приложения.
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation_mask(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
            NSDragOperation::Copy
        }
    }
);

impl Source {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        // SAFETY: `init` — назначенный инициализатор `NSObject`.
        unsafe { msg_send![this, init] }
    }
}

thread_local! {
    /// Источник живёт до конца процесса: сеанс держит его слабо.
    static SOURCE: OnceCell<Retained<Source>> = const { OnceCell::new() };
}

/// Порог начала перетаскивания в точках окна (у AppKit своего числа нет; так же ведут себя Finder и Mail).
pub fn threshold(_scale: f32) -> (f32, f32) {
    (4., 4.)
}

/// Перетащить файлы `paths` (копии на диске) из ключевого окна приложения. Главный поток, во время события мыши.
/// Сеанс идёт дальше сам: `Ok(())` — начат.
pub fn drag_out(paths: &[PathBuf]) -> io::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mtm = MainThreadMarker::new().ok_or_else(|| io::Error::other("drag: not the main thread"))?;
    let app = NSApplication::sharedApplication(mtm);
    let event = app
        .currentEvent()
        .ok_or_else(|| io::Error::other("drag: no current event"))?;
    if !matches!(
        event.r#type(),
        NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged
    ) {
        return Err(io::Error::other("drag: not during a mouse drag"));
    }
    let view = app
        .keyWindow()
        .and_then(|w| w.contentView())
        .ok_or_else(|| io::Error::other("drag: no key window"))?;
    let at = view.convertPoint_fromView(event.locationInWindow(), None);
    let workspace = NSWorkspace::sharedWorkspace();
    let mut items = Vec::with_capacity(paths.len());
    for (i, path) in paths.iter().enumerate() {
        let path = NSString::from_str(&path.to_string_lossy());
        let url = NSURL::fileURLWithPath(&path);
        let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*url));
        let icon = workspace.iconForFile(&path);
        let shift = 8. * i as f64;
        let frame = NSRect::new(
            NSPoint::new(at.x - 16. + shift, at.y - 16. - shift),
            NSSize::new(32., 32.),
        );
        let contents: &AnyObject = &icon;
        // SAFETY: `contents` — NSImage, как требует `setDraggingFrame:contents:`.
        unsafe { item.setDraggingFrame_contents(frame, Some(contents)) };
        items.push(item);
    }
    let items = NSArray::from_retained_slice(&items);
    SOURCE.with(|s| {
        let source = s.get_or_init(|| Source::new(mtm));
        view.beginDraggingSessionWithItems_event_source(&items, &event, ProtocolObject::from_ref(&**source));
    });
    Ok(())
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
    /// Копии — в своих подкаталогах; чистка убирает всё, повторная — не ошибка.
    #[test]
    fn temp_copies_are_cleaned() {
        let root = std::env::temp_dir().join(format!("pets-drag-test-{}", std::process::id()));
        let copies = super::TempCopies::at(root.clone());
        let (a, b) = (copies.slot().unwrap(), copies.slot().unwrap());
        assert!(a != b && a.starts_with(&root) && b.is_dir());
        copies.clean().unwrap();
        assert!(!root.exists());
        copies.clean().unwrap();
    }

    /// Не во время события мыши (тест без окна и цикла событий) — ошибка, а не исключение AppKit.
    #[test]
    fn drag_without_mouse_event_is_an_error() {
        if objc2::MainThreadMarker::new().is_some() {
            assert!(super::drag_out(&[std::env::temp_dir()]).is_err());
        }
    }
}
