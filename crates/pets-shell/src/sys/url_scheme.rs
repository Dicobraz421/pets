//! macOS: приложение по умолчанию для схемы адресов (`mailto`) — LaunchServices. Схема должна быть объявлена в
//! `Info.plist` приложения (`CFBundleURLTypes`); выбор — для этого пользователя. macOS 12+ сама спрашивает
//! подтверждение («Использовать … для электронной почты?»).

use core_foundation::base::TCFType as _;
use core_foundation::bundle::CFBundle;
use core_foundation::string::{CFString, CFStringRef};

use crate::ShellError;

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn LSSetDefaultHandlerForURLScheme(scheme: CFStringRef, bundle_id: CFStringRef) -> i32;
    fn LSCopyDefaultHandlerForURLScheme(scheme: CFStringRef) -> CFStringRef;
}

/// Идентификатор пакета этого процесса (`CFBundleIdentifier`); не из `.app` (`cargo run`) — `None`.
pub fn bundle_id() -> Option<String> {
    let info = CFBundle::main_bundle().info_dictionary();
    let id = info.find(CFString::from_static_string("CFBundleIdentifier"))?;
    id.downcast::<CFString>().map(|s| s.to_string())
}

/// Сделать это приложение обработчиком схемы `scheme` по умолчанию.
pub fn set_default(scheme: &str) -> Result<(), ShellError> {
    let id = bundle_id().ok_or(ShellError::Invalid {
        what: "LSSetDefaultHandlerForURLScheme",
        reason: "not running from an app bundle",
    })?;
    let (scheme, id) = (CFString::new(scheme), CFString::new(&id));
    // SAFETY: обе строки живы до конца вызова; функция их не забирает.
    let status = unsafe { LSSetDefaultHandlerForURLScheme(scheme.as_concrete_TypeRef(), id.as_concrete_TypeRef()) };
    if status == 0 {
        Ok(())
    } else {
        Err(ShellError::Os {
            what: "LSSetDefaultHandlerForURLScheme",
            source: std::io::Error::other(format!("OSStatus {status}")),
        })
    }
}

/// Идентификатор пакета обработчика схемы по умолчанию; нет — `None`.
pub fn default_handler(scheme: &str) -> Option<String> {
    let scheme = CFString::new(scheme);
    // SAFETY: строка жива до конца вызова; результат — по правилу Copy (наш).
    let id = unsafe { LSCopyDefaultHandlerForURLScheme(scheme.as_concrete_TypeRef()) };
    // SAFETY: ненулевой результат — CFString с одной нашей ссылкой, её забирает `wrap_under_create_rule`.
    (!id.is_null()).then(|| unsafe { CFString::wrap_under_create_rule(id) }.to_string())
}

/// Обработчик схемы по умолчанию — это приложение (без учёта регистра, как в LaunchServices).
pub fn is_default(scheme: &str) -> bool {
    match (bundle_id(), default_handler(scheme)) {
        (Some(me), Some(h)) => me.eq_ignore_ascii_case(&h),
        _ => false,
    }
}
