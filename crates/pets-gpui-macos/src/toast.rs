//! Уведомление центра уведомлений macOS (`UNUserNotificationCenter`) от имени приложения. Нужен `.app` с
//! идентификатором пакета: вне пакета (`cargo run`) фреймворк завершает процесс исключением — тогда ошибка, без
//! вызова. Разрешение спрашивается при первом уведомлении (один раз за запуск); отказ пользователя — уведомления
//! молча не показываются. Баннер — и когда приложение впереди (делегат `willPresentNotification`).
//!
//! Свой делегат ставится, только если приложение не пользуется уведомлениями gpui (`App::show_system_notification`):
//! делегат у центра один.

use std::io;
use std::sync::Once;
use std::sync::atomic::{AtomicU64, Ordering};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, define_class, msg_send};
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

/// Звук уведомления.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sound {
    /// Звук уведомлений по умолчанию.
    #[default]
    Default,
    /// Без звука: уведомление показывается молча.
    Silent,
}

define_class!(
    // SAFETY: у `NSObject` нет требований к наследникам; `Drop` нет.
    #[unsafe(super(NSObject))]
    #[name = "PetsToastDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// Без этого macOS прячет баннер, пока приложение впереди; показывать ли — решает приложение.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }
    }
);

impl Delegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc();
        // SAFETY: `init` — назначенный инициализатор `NSObject`.
        unsafe { msg_send![this, init] }
    }
}

/// Делегат и запрос разрешения — один раз за запуск.
fn prepare(center: &UNUserNotificationCenter) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Свойство `delegate` слабое: делегат живёт до конца процесса.
        let delegate = Delegate::new();
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        std::mem::forget(delegate);
        let done = RcBlock::new(|granted: Bool, error: *mut NSError| {
            // SAFETY: ненулевой `error` — живой NSError на время вызова.
            if let Some(error) = unsafe { error.as_ref() } {
                tracing::warn!(target: "pets_gpui_macos", "notification authorization: {}", error.localizedDescription());
            } else if !granted.as_bool() {
                tracing::info!(target: "pets_gpui_macos", "notifications are not allowed by the user");
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &done,
        );
    });
}

/// Показать уведомление: заголовок и текст, звук по умолчанию.
pub fn show(title: &str, body: &str) -> io::Result<()> {
    show_with(title, body, Sound::Default)
}

/// Показать уведомление со своим звуком. Любой поток.
pub fn show_with(title: &str, body: &str, sound: Sound) -> io::Result<()> {
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "notifications need an app bundle",
        ));
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    prepare(&center);
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    if sound == Sound::Default {
        content.setSound(Some(&UNNotificationSound::defaultSound()));
    }
    // Свой идентификатор у каждого уведомления: новое не заменяет прежнее.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = format!(
        "pets-toast-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let request =
        UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&id), &content, None);
    let done = RcBlock::new(|error: *mut NSError| {
        // SAFETY: ненулевой `error` — живой NSError на время вызова.
        if let Some(error) = unsafe { error.as_ref() } {
            tracing::warn!(target: "pets_gpui_macos", "notification is not delivered: {}", error.localizedDescription());
        }
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&done));
    Ok(())
}
