//! Уведомление центра уведомлений macOS (`UNUserNotificationCenter`) от имени приложения. Нужен `.app` с
//! идентификатором пакета: вне пакета (`cargo run`) фреймворк завершает процесс исключением — тогда ошибка, без
//! вызова. Разрешение спрашивается при первом уведомлении (один раз за запуск); отказ пользователя — уведомления
//! молча не показываются. Баннер — и когда приложение впереди (делегат `willPresentNotification`).
//!
//! Щелчок (Postman MAC-04): [`show_clickable`] — обработчик по идентификатору уведомления, делегат
//! `didReceiveNotificationResponse` зовёт его, пока приложение запущено (как `show_clickable` на Windows). Помнятся
//! последние [`MAX_CLICKS`] обработчиков: старые уведомления центр всё равно убирает.
//!
//! Свой делегат ставится, только если приложение не пользуется уведомлениями gpui (`App::show_system_notification`):
//! делегат у центра один.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once, PoisonError};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, define_class, msg_send};
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNNotificationSound, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};

/// Обработчик щелчка по уведомлению.
type OnClick = Box<dyn Fn() + Send>;

/// Сколько обработчиков щелчка помнить.
pub const MAX_CLICKS: usize = 64;

/// Обработчики щелчка по идентификатору уведомления, старые — в начале.
static CLICKS: Mutex<VecDeque<(String, OnClick)>> = Mutex::new(VecDeque::new());

/// Запомнить обработчик уведомления `id`; лишние старые — забыть.
fn remember(id: String, on_click: OnClick) {
    let mut clicks = CLICKS.lock().unwrap_or_else(PoisonError::into_inner);
    while clicks.len() >= MAX_CLICKS {
        clicks.pop_front();
    }
    clicks.push_back((id, on_click));
}

/// Обработчик уведомления `id` (один раз: щелчок убирает уведомление).
fn take(id: &str) -> Option<OnClick> {
    let mut clicks = CLICKS.lock().unwrap_or_else(PoisonError::into_inner);
    let at = clicks.iter().position(|(i, _)| i == id)?;
    clicks.remove(at).map(|(_, f)| f)
}

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

        /// Щелчок по уведомлению (или его кнопке): обработчик из [`show_clickable`], если он есть.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            let id = response.notification().request().identifier().to_string();
            if let Some(on_click) = take(&id) {
                on_click();
            }
            completion.call(());
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
    post(title, body, sound, None)
}

/// Показать уведомление; щелчок по нему зовёт `on_click` (в потоке центра уведомлений, пока приложение запущено).
pub fn show_clickable(title: &str, body: &str, sound: Sound, on_click: impl Fn() + Send + 'static) -> io::Result<()> {
    post(title, body, sound, Some(Box::new(on_click)))
}

fn post(title: &str, body: &str, sound: Sound, on_click: Option<OnClick>) -> io::Result<()> {
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
    if let Some(on_click) = on_click {
        remember(id, on_click);
    }
    let done = RcBlock::new(|error: *mut NSError| {
        // SAFETY: ненулевой `error` — живой NSError на время вызова.
        if let Some(error) = unsafe { error.as_ref() } {
            tracing::warn!(target: "pets_gpui_macos", "notification is not delivered: {}", error.localizedDescription());
        }
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&done));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Обработчик щелчка — один раз; сверх [`super::MAX_CLICKS`] старые забываются.
    #[test]
    fn clicks_are_remembered_once() {
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        super::remember("test-a".into(), Box::new(move || _ = h.fetch_add(1, Ordering::Relaxed)));
        super::take("test-a").unwrap()();
        assert!(super::take("test-a").is_none());
        assert_eq!(hits.load(Ordering::Relaxed), 1);
        for i in 0..=super::MAX_CLICKS {
            super::remember(format!("test-{i}"), Box::new(|| {}));
        }
        assert!(super::take("test-0").is_none());
        assert!(super::take(&format!("test-{}", super::MAX_CLICKS)).is_some());
    }
}
