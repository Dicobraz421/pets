//! Номер окна и снимок окна. Свои окна процесс снимает без разрешения «Запись экрана».
//!
//! Снимок (Postman MAC-04): macOS 14.4 и новее — ScreenCaptureKit (`SCShareableContent` своего процесса,
//! `SCScreenshotManager`); macOS 13 — 14.3 и при отказе ScreenCaptureKit — `CGWindowListCreateImage` (устарела с
//! macOS 14, но работает). Номер окна — `-[NSWindow windowNumber]` и `CGWindowListCopyWindowInfo`.

use std::cell::Cell;
use std::ffi::c_void;
use std::io;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread as _, MainThreadMarker, Message as _, class, msg_send};
use objc2_app_kit::NSView;
use objc2_core_graphics::CGImage;
use objc2_foundation::NSError;
use objc2_screen_capture_kit::{
    SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration, SCWindow,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Сколько ждать ответа ScreenCaptureKit (обычно — десятки миллисекунд).
const SCK_TIMEOUT: Duration = Duration::from_secs(5);

type CFTypeRef = *const c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFArrayGetCount(array: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
    fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFNumberGetValue(number: CFTypeRef, kind: isize, value: *mut c_void) -> u8;
    fn CFRelease(cf: CFTypeRef);
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    static kCGWindowNumber: CFTypeRef;
    static kCGWindowOwnerPID: CFTypeRef;
    static kCGWindowLayer: CFTypeRef;
    static CGRectNull: CGRect;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
    fn CGWindowListCreateImage(bounds: CGRect, option: u32, window: u32, image_option: u32) -> CFTypeRef;
    fn CGImageGetWidth(image: CFTypeRef) -> usize;
    fn CGImageGetHeight(image: CFTypeRef) -> usize;
    fn CGColorSpaceCreateDeviceRGB() -> CFTypeRef;
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits: usize,
        row: usize,
        space: CFTypeRef,
        info: u32,
    ) -> CFTypeRef;
    fn CGContextDrawImage(context: CFTypeRef, rect: CGRect, image: CFTypeRef);
}

const ON_SCREEN_ONLY: u32 = 1 << 0;
const EXCLUDE_DESKTOP: u32 = 1 << 4;
const INCLUDING_WINDOW: u32 = 1 << 3;
/// Без тени окна.
const BOUNDS_IGNORE_FRAMING: u32 = 1 << 0;
/// `kCFNumberSInt64Type`.
const SINT64: isize = 4;
/// `kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big`: байты R, G, B, A.
const RGBA: u32 = 1 | (4 << 12);

/// Снимок окна: RGBA, 8 бит на канал, строки сверху вниз.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Объект CoreFoundation, освобождается при выходе из области.
struct Owned(CFTypeRef);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: объект получен от функции Create/Copy и освобождается один раз.
            unsafe { CFRelease(self.0) }
        }
    }
}

/// Номер окна (`CGWindowID` = `-[NSWindow windowNumber]`) — его можно передать в другой поток (сам `Window` туда не
/// передаётся). Вызывать в главном потоке.
pub fn id(window: &impl HasWindowHandle) -> io::Result<isize> {
    let handle = window
        .window_handle()
        .map_err(|e| io::Error::other(format!("window handle: {e}")))?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "not an AppKit window"));
    };
    if MainThreadMarker::new().is_none() {
        return Err(io::Error::other("window id: not the main thread"));
    }
    // SAFETY: `ns_view` — живой NSView окна, пока жив `window` (заимствован на время вызова); главный поток.
    let view = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let number = view
        .window()
        .ok_or_else(|| io::Error::other("view without a window"))?
        .windowNumber();
    if number <= 0 {
        return Err(io::Error::other("window is not on screen"));
    }
    Ok(number)
}

/// Попросить систему перерисовать окно — стандартно для AppKit: `setNeedsDisplay:` у вида окна и
/// `-[NSWindow displayIfNeeded]`. AppKit зовёт `displayLayer:`, и gpui рисует кадр синхронно, не дожидаясь своей
/// ссылки кадров (GMAC-01: на виртуальном дисплее CI `CAMetalDisplayLink` надолго замолкает). Делается на ближайшем
/// проходе главной очереди (`NSOperationQueue.mainQueue` будит цикл), вне текущего обновления gpui; повторные вызовы
/// до него сливаются в один. Вызывать в главном потоке.
pub fn redraw(window: &impl HasWindowHandle) -> io::Result<()> {
    thread_local! {
        static PENDING: Cell<bool> = const { Cell::new(false) };
    }
    let handle = window
        .window_handle()
        .map_err(|e| io::Error::other(format!("window handle: {e}")))?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "not an AppKit window"));
    };
    if MainThreadMarker::new().is_none() {
        return Err(io::Error::other("redraw: not the main thread"));
    }
    if PENDING.replace(true) {
        return Ok(());
    }
    // SAFETY: `ns_view` — живой NSView окна, пока жив `window` (заимствован на время вызова); блок держит его копию.
    let view = unsafe { h.ns_view.cast::<NSView>().as_ref() }.retain();
    let block = RcBlock::new(move || {
        PENDING.set(false);
        view.setNeedsDisplay(true);
        if let Some(window) = view.window() {
            window.displayIfNeeded();
        }
    });
    // SAFETY: `+[NSOperationQueue mainQueue]` и `-addOperationWithBlock:` (блок копирует очередь); главная очередь
    // исполняет блок в главном потоке, где и живёт вид.
    unsafe {
        let queue: *mut AnyObject = msg_send![class!(NSOperationQueue), mainQueue];
        let _: () = msg_send![queue, addOperationWithBlock: &*block];
    }
    Ok(())
}

/// Число из словаря окна.
fn number(dict: CFTypeRef, key: CFTypeRef) -> Option<i64> {
    // SAFETY: `dict` — словарь из живого массива окон, ключ — константа CoreGraphics.
    let value = unsafe { CFDictionaryGetValue(dict, key) };
    if value.is_null() {
        return None;
    }
    let mut out = 0i64;
    // SAFETY: значение по этим ключам — CFNumber; пишем в локальную переменную подходящего размера.
    (unsafe { CFNumberGetValue(value, SINT64, (&raw mut out).cast()) } != 0).then_some(out)
}

/// Номер окна этого процесса на экране, впереди всех (слой 0 — обычные окна): без `Window` (другой поток, снимки).
pub fn frontmost_own() -> io::Result<isize> {
    let pid = i64::from(std::process::id());
    // SAFETY: функция без указателей; массив освобождает `Owned`.
    let list = Owned(unsafe { CGWindowListCopyWindowInfo(ON_SCREEN_ONLY | EXCLUDE_DESKTOP, 0) });
    if list.0.is_null() {
        return Err(io::Error::other("CGWindowListCopyWindowInfo failed"));
    }
    // SAFETY: `list.0` — живой CFArray словарей окон (порядок — спереди назад); ключи — константы CoreGraphics.
    let (count, keys) = unsafe {
        (
            CFArrayGetCount(list.0),
            (kCGWindowOwnerPID, kCGWindowLayer, kCGWindowNumber),
        )
    };
    for i in 0..count {
        // SAFETY: индекс в пределах массива.
        let dict = unsafe { CFArrayGetValueAtIndex(list.0, i) };
        if number(dict, keys.0) == Some(pid)
            && number(dict, keys.1) == Some(0)
            && let Some(id) = number(dict, keys.2).and_then(|n| isize::try_from(n).ok())
        {
            return Ok(id);
        }
    }
    Err(io::Error::other("no on-screen window of this process"))
}

/// Снимок окна `window` ([`id`]) без тени: RGBA, строки сверху вниз. Не из главного потока: ScreenCaptureKit
/// отвечает в своей очереди, вызов ждёт ответа.
pub fn capture(window: isize) -> io::Result<Image> {
    let id = u32::try_from(window).map_err(|_| io::Error::other(format!("window id {window}")))?;
    if objc2::available!(macos = 14.4) {
        match capture_sck(id) {
            Ok(image) => return Ok(image),
            Err(e) => tracing::warn!(target: "pets_gpui_macos", "ScreenCaptureKit: {e}; CoreGraphics capture"),
        }
    }
    capture_cg(id)
}

/// Ошибка из `NSError` обработчика (или «нет результата»).
fn ns_error(error: *mut NSError, what: &str) -> io::Error {
    // SAFETY: ненулевой `error` — живой NSError на время обработчика.
    match unsafe { error.as_ref() } {
        Some(e) => io::Error::other(format!("{what}: {}", e.localizedDescription())),
        None => io::Error::other(format!("{what}: no result")),
    }
}

/// ScreenCaptureKit (macOS 14.4+): окно `id` среди окон своего процесса — без разрешения «Запись экрана»; снимок —
/// в пикселях экрана, без тени и курсора.
fn capture_sck(id: u32) -> io::Result<Image> {
    let (tx, rx) = mpsc::channel::<io::Result<Image>>();
    let on_content = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        // SAFETY: ненулевой `content` — живой объект на время обработчика.
        let started = match unsafe { content.as_ref() } {
            Some(content) => screenshot(content, id, tx.clone()),
            None => Err(ns_error(error, "shareable content")),
        };
        if let Err(e) = started {
            let _ = tx.send(Err(e));
        }
    });
    // SAFETY: обработчик — блок с подписью, которую ждёт метод; держит только `Sender`.
    unsafe { SCShareableContent::getCurrentProcessShareableContentWithCompletionHandler(&on_content) };
    rx.recv_timeout(SCK_TIMEOUT)
        .map_err(|_| io::Error::other(format!("no answer in {} s", SCK_TIMEOUT.as_secs())))?
}

/// Запросить снимок окна `id` из `content`; итог — в `tx`.
fn screenshot(content: &SCShareableContent, id: u32, tx: mpsc::Sender<io::Result<Image>>) -> io::Result<()> {
    // SAFETY: свойства живых объектов ScreenCaptureKit.
    let window: Retained<SCWindow> = unsafe { content.windows() }
        .iter()
        .find(|w| unsafe { w.windowID() } == id)
        .ok_or_else(|| io::Error::other(format!("window {id} is not in the shareable content")))?;
    // SAFETY: назначенный инициализатор фильтра одного окна; окно живо.
    let filter = unsafe { SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &window) };
    // SAFETY: свойства фильтра (macOS 14+) и настройки снимка — простые значения.
    let config = unsafe {
        let rect = filter.contentRect();
        let scale = f64::from(filter.pointPixelScale());
        let config = SCStreamConfiguration::new();
        config.setWidth((rect.size.width * scale).round() as usize);
        config.setHeight((rect.size.height * scale).round() as usize);
        config.setShowsCursor(false);
        config.setIgnoreShadowsSingleWindow(true);
        config
    };
    let on_image = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
        let result = if image.is_null() {
            Err(ns_error(error, "screenshot"))
        } else {
            draw(image.cast_const().cast())
        };
        let _ = tx.send(result);
    });
    // SAFETY: фильтр и настройки живы до конца вызова (метод их удерживает); блок с ожидаемой подписью.
    unsafe {
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(&filter, &config, Some(&on_image));
    }
    Ok(())
}

/// CoreGraphics (`CGWindowListCreateImage`): macOS 13 — 14.3 и запасной путь.
fn capture_cg(id: u32) -> io::Result<Image> {
    // SAFETY: `CGRectNull` — константа CoreGraphics; снимок освобождает `Owned`.
    let image = Owned(unsafe { CGWindowListCreateImage(CGRectNull, INCLUDING_WINDOW, id, BOUNDS_IGNORE_FRAMING) });
    if image.0.is_null() {
        return Err(io::Error::other(format!("CGWindowListCreateImage({id}) failed")));
    }
    draw(image.0)
}

/// Пиксели `CGImage` в RGBA (8 бит на канал, строки сверху вниз). `image` — живой CGImage на время вызова.
fn draw(image: CFTypeRef) -> io::Result<Image> {
    // SAFETY: `image` — живой CGImage (обещание вызывающего).
    let (width, height) = unsafe { (CGImageGetWidth(image), CGImageGetHeight(image)) };
    let (Ok(w), Ok(h)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(io::Error::other(format!("capture {width}x{height}")));
    };
    if w == 0 || h == 0 {
        return Err(io::Error::other("capture is empty"));
    }
    let overflow = || io::Error::other("capture size overflow");
    let row = width.checked_mul(4).ok_or_else(overflow)?;
    let mut rgba = vec![0u8; row.checked_mul(height).ok_or_else(overflow)?];
    // SAFETY: буфер `rgba` — ровно `row × height` байт и живёт дольше контекста; контекст и пространство цветов
    // освобождает `Owned` (контекст — первым: объявлен позже).
    unsafe {
        let space = Owned(CGColorSpaceCreateDeviceRGB());
        let ctx = Owned(CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            width,
            height,
            8,
            row,
            space.0,
            RGBA,
        ));
        if ctx.0.is_null() {
            return Err(io::Error::other("CGBitmapContextCreate failed"));
        }
        let rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: width as f64,
                height: height as f64,
            },
        };
        CGContextDrawImage(ctx.0, rect, image);
    }
    // Окно непрозрачно: предумноженная альфа совпадает с обычной.
    Ok(Image {
        width: w,
        height: h,
        rgba,
    })
}
