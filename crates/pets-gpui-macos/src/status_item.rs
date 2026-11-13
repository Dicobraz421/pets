//! Значок приложения в строке меню (`NSStatusItem` в `NSStatusBar.systemStatusBar`) — аналог значка в трее на
//! Windows (`gpui_windows::pets::tray`): монохромный значок-шаблон, рядом короткий текст (число непрочитанных), меню
//! из пунктов приложения. Щелчок по пункту зовёт обработчик с номером пункта.
//!
//! Значок — шаблон (`isTemplate`): система берёт только прозрачность картинки и красит её под тему строки меню
//! (светлая, тёмная, выделение). Только главный поток (поток интерфейса gpui): [`StatusItem::new`] вне его — ошибка.
//! Значок живёт, пока жив [`StatusItem`]; `Drop` убирает его из строки меню.

use std::io;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{AnyThread as _, DefinedClass as _, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSCellImagePosition, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSSize, NSString};

/// Сторона значка в точках: так рисуют свои значки системные приложения (строка меню — 22–24 точки).
pub const ICON_SIZE: f64 = 18.;

/// Пункт меню значка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// Номер (его получает обработчик) и текст.
    Command(u32, String),
    Separator,
}

/// Обработчик щелчка по пункту меню: номер пункта.
type OnMenu = Box<dyn Fn(u32)>;

define_class!(
    // SAFETY: у `NSObject` нет требований к наследникам; `Drop` нет, обработчик — в ivars.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PetsStatusItemTarget"]
    #[ivars = OnMenu]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        /// Действие пунктов меню: номер пункта — его `tag`.
        #[unsafe(method(petsStatusItemMenu:))]
        fn menu(&self, sender: &NSMenuItem) {
            if let Ok(id) = u32::try_from(sender.tag()) {
                (self.ivars())(id);
            }
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker, on_menu: OnMenu) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(on_menu);
        // SAFETY: `init` — назначенный инициализатор `NSObject`.
        unsafe { msg_send![super(this), init] }
    }
}

/// Значок в строке меню. Не `Send`: только главный поток.
pub struct StatusItem {
    item: Retained<NSStatusItem>,
    /// Цель действий пунктов меню: `NSMenuItem.target` — слабая ссылка, держим здесь.
    target: Retained<Target>,
    mtm: MainThreadMarker,
}

impl StatusItem {
    /// Поставить значок: `image` — картинка в любом формате `NSImage` (PDF, PNG, TIFF; лучше PDF или PNG вдвое
    /// крупнее [`ICON_SIZE`]), рисуется шаблоном размером [`ICON_SIZE`]; `tip` — подсказка; `on_menu` — щелчок по
    /// пункту [`Item::Command`] (главный поток, во время работы меню). Меню — [`StatusItem::set_menu`].
    pub fn new(image: &[u8], tip: &str, on_menu: impl Fn(u32) + 'static) -> io::Result<Self> {
        let mtm = MainThreadMarker::new().ok_or_else(|| io::Error::other("status item: not the main thread"))?;
        let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(image))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "status item: image is not readable"))?;
        image.setSize(NSSize::new(ICON_SIZE, ICON_SIZE));
        image.setTemplate(true);
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        let this = Self {
            item,
            target: Target::new(mtm, Box::new(on_menu)),
            mtm,
        };
        let button = this
            .item
            .button(mtm)
            .ok_or_else(|| io::Error::other("status item: no button"))?;
        button.setImage(Some(&image));
        button.setImagePosition(NSCellImagePosition::ImageOnly);
        this.set_tip(tip);
        Ok(this)
    }

    /// Число рядом со значком: `Some(n)` — `n`, `None` или `Some(0)` — только значок.
    pub fn set_count(&self, count: Option<u32>) {
        self.set_title(label(count).as_deref());
    }

    /// Свой короткий текст рядом со значком («999+» и т. п.); `None` или пустая строка — только значок.
    pub fn set_title(&self, text: Option<&str>) {
        let Some(button) = self.item.button(self.mtm) else {
            return;
        };
        let text = text.unwrap_or_default();
        button.setTitle(&NSString::from_str(text));
        button.setImagePosition(if text.is_empty() {
            NSCellImagePosition::ImageOnly
        } else {
            NSCellImagePosition::ImageLeft
        });
    }

    /// Подсказка при наведении (и название значка для VoiceOver).
    pub fn set_tip(&self, tip: &str) {
        if let Some(button) = self.item.button(self.mtm) {
            button.setToolTip(Some(&NSString::from_str(tip)));
        }
    }

    /// Меню по щелчку на значок; пустой список — без меню.
    pub fn set_menu(&self, items: &[Item]) {
        if items.is_empty() {
            self.item.setMenu(None);
            return;
        }
        let menu = NSMenu::new(self.mtm);
        menu.setAutoenablesItems(false);
        for item in items {
            let entry = match item {
                Item::Separator => NSMenuItem::separatorItem(self.mtm),
                Item::Command(id, text) => {
                    // SAFETY: селектор — метод `Target` (`define_class!` выше) с одним аргументом-объектом.
                    let entry = unsafe {
                        NSMenuItem::initWithTitle_action_keyEquivalent(
                            NSMenuItem::alloc(self.mtm),
                            &NSString::from_str(text),
                            Some(sel!(petsStatusItemMenu:)),
                            &NSString::new(),
                        )
                    };
                    // SAFETY: цель отвечает на селектор пункта и живёт, пока жив `self` (меню — тоже у `self.item`).
                    let target: &AnyObject = &self.target;
                    unsafe { entry.setTarget(Some(target)) };
                    entry.setTag(*id as isize);
                    entry.setEnabled(true);
                    entry
                }
            };
            menu.addItem(&entry);
        }
        self.item.setMenu(Some(&menu));
    }
}

impl Drop for StatusItem {
    fn drop(&mut self) {
        // Меню держит слабую ссылку на цель: сначала меню, потом сам значок.
        self.item.setMenu(None);
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}

/// Текст рядом со значком для числа: 0 и `None` — без текста.
fn label(count: Option<u32>) -> Option<String> {
    count.filter(|&n| n > 0).map(|n| n.to_string())
}

#[cfg(test)]
mod tests {
    use super::label;

    #[test]
    fn status_item_label_hides_zero() {
        assert_eq!(label(None), None);
        assert_eq!(label(Some(0)), None);
        assert_eq!(label(Some(12)).as_deref(), Some("12"));
    }
}
