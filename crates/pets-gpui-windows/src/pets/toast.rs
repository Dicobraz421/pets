//! Уведомление центра уведомлений Windows от имени приложения без ярлыка в «Пуске»: AUMID регистрируется в
//! `HKCU\Software\Classes\AppUserModelId\<aumid>` (имя и значок), процессу AUMID не ставится — группировка окон на
//! панели задач не меняется (в отличие от `App::set_app_identity` gpui).

use std::io;
use std::path::Path;

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::HSTRING;

use windows_registry::CURRENT_USER;

use super::com_error;

/// От чьего имени уведомление.
pub struct Sender<'a> {
    /// AUMID: `Pets.Postman`.
    pub aumid: &'a str,
    /// Имя в заголовке уведомления.
    pub name: &'a str,
    /// Значок — файл картинки на диске (`IconUri` у AUMID — путь к файлу, не ресурс exe); нет — значок по умолчанию.
    pub icon: Option<&'a Path>,
}

/// Звук уведомления (`<audio>` ToastGeneric).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sound {
    /// Звук уведомлений Windows по умолчанию.
    #[default]
    Default,
    /// Звук «Новое письмо» (`Notification.Mail`).
    Mail,
    /// Без звука: уведомление показывается молча.
    Silent,
}

/// Показать уведомление: заголовок и текст, звук по умолчанию.
pub fn show(sender: &Sender<'_>, title: &str, body: &str) -> io::Result<()> {
    show_with(sender, title, body, Sound::Default)
}

/// Показать уведомление со своим звуком (Postman: «Звук „Новое письмо“», выключен — молча).
pub fn show_with(sender: &Sender<'_>, title: &str, body: &str, sound: Sound) -> io::Result<()> {
    let key = format!(r"Software\Classes\AppUserModelId\{}", sender.aumid);
    let register = || -> windows::core::Result<()> {
        let key = CURRENT_USER.create(&key)?;
        key.set_string("DisplayName", sender.name)?;
        if let Some(icon) = sender.icon.filter(|p| p.exists()) {
            key.set_string("IconUri", icon.display().to_string())?;
        }
        Ok(())
    };
    register().map_err(|e| com_error("AppUserModelId", e))?;
    let xml = document(title, body, sound);
    let run = || -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml.as_str()))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(sender.aumid))?.Show(&toast)
    };
    run().map_err(|e| com_error("toast", e))
}

/// XML уведомления ToastGeneric: две строки, текст экранирован; звук — `<audio>`.
fn document(title: &str, body: &str, sound: Sound) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let audio = match sound {
        Sound::Default => "",
        Sound::Mail => "<audio src=\"ms-winsoundevent:Notification.Mail\"/>",
        Sound::Silent => "<audio silent=\"true\"/>",
    };
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual>{audio}</toast>",
        esc(title),
        esc(body)
    )
}

#[cfg(test)]
mod tests {
    /// Разметка из текста пользователя не ломает XML уведомления.
    #[test]
    fn pets_toast_text_is_escaped() {
        let xml = super::document("A & <b>", "\"x\" > y", super::Sound::Default);
        assert!(
            xml.contains("<text>A &amp; &lt;b&gt;</text><text>&quot;x&quot; &gt; y</text>"),
            "{xml}"
        );
        let doc = windows::Data::Xml::Dom::XmlDocument::new().unwrap();
        doc.LoadXml(&windows::core::HSTRING::from(xml.as_str())).unwrap();
    }

    /// Звук: по умолчанию — без `<audio>`, почта — звук «Новое письмо», молча — `silent`; разметка разбирается.
    #[test]
    fn pets_toast_sound() {
        use super::{Sound, document};
        assert!(!document("a", "b", Sound::Default).contains("<audio"));
        assert!(document("a", "b", Sound::Mail).contains("ms-winsoundevent:Notification.Mail"));
        let silent = document("a", "b", Sound::Silent);
        assert!(silent.ends_with("<audio silent=\"true\"/></toast>"), "{silent}");
        let doc = windows::Data::Xml::Dom::XmlDocument::new().unwrap();
        doc.LoadXml(&windows::core::HSTRING::from(silent.as_str())).unwrap();
    }
}
