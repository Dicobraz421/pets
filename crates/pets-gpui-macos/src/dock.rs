//! Значок приложения в Dock: бейдж (`NSDockTile.badgeLabel`) — например, число непрочитанных писем, как подсказка
//! значка в трее на Windows. Только из главного потока (поток интерфейса gpui); из другого — ошибка, без вызова.

use std::io;

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::NSString;

/// Поставить бейдж на значок в Dock: `Some(n)` — число `n`, `None` или `Some(0)` — убрать бейдж.
pub fn set_badge(count: Option<u32>) -> io::Result<()> {
    set_badge_label(label(count).as_deref())
}

/// Поставить на значок в Dock свой текст бейджа (`"999+"` и т. п.); `None` или пустая строка — убрать бейдж.
pub fn set_badge_label(text: Option<&str>) -> io::Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| io::Error::other("dock: not the main thread"))?;
    let tile = NSApplication::sharedApplication(mtm).dockTile();
    let text = text.filter(|t| !t.is_empty()).map(NSString::from_str);
    tile.setBadgeLabel(text.as_deref());
    Ok(())
}

/// Текст бейджа для числа: 0 и `None` — без бейджа.
fn label(count: Option<u32>) -> Option<String> {
    count.filter(|&n| n > 0).map(|n| n.to_string())
}

#[cfg(test)]
mod tests {
    use super::label;

    #[test]
    fn dock_badge_label_hides_zero() {
        assert_eq!(label(None), None);
        assert_eq!(label(Some(0)), None);
        assert_eq!(label(Some(7)).as_deref(), Some("7"));
    }
}
