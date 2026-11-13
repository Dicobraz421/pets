//! Pets patch: источник XDND — перетаскивание файлов из окна в другие приложения на X11.
//!
//! gpui сам переводит внутреннее перетаскивание во внешнее, когда указатель уходит за окно
//! (`Div::external_drag_payload` → `PlatformWindow::start_external_drag`); upstream это есть только на
//! Wayland. Здесь — X11: владелец выделения `XdndSelection` — окно-источник, формат один — `text/uri-list`,
//! действие — `XdndActionCopy`. События указателя приходят окну-источнику по неявному захвату кнопки
//! (XInput2), поэтому свой захват не нужен. Свои окна целью не считаются: бросок в себя — отмена.

use url::Url;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{
    self, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, PropMode, SelectionNotifyEvent,
    SelectionRequestEvent,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::xcb_ffi::XCBConnection;

use gpui::ExternalDragPayload;

use super::XcbAtoms;

/// Наша версия протокола XDND.
const XDND_VERSION: u32 = 5;
/// Глубина спуска по дереву окон в поисках `XdndAware`.
const MAX_DEPTH: usize = 16;

/// `text/uri-list` (RFC 2483, строки через CRLF) для путей файлов; пути, которые не стали `file://`, — мимо.
pub(crate) fn uri_list(payload: &ExternalDragPayload) -> Vec<u8> {
    let ExternalDragPayload::Files(paths) = payload;
    let mut list = String::new();
    for (path, _) in paths.entries() {
        if let Ok(url) = Url::from_file_path(path) {
            list.push_str(url.as_str());
            list.push_str("\r\n");
        }
    }
    list.into_bytes()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Target {
    window: xproto::Window,
    version: u32,
    accepted: bool,
}

/// Сеанс перетаскивания, начатый нашим окном.
#[derive(Debug)]
pub(crate) struct PetsXdnd {
    /// Окно-источник (наше): ему приходят `XdndStatus`, `XdndFinished` и запросы выделения.
    pub(crate) source: xproto::Window,
    uri_list: Vec<u8>,
    target: Option<Target>,
    /// Ждём `XdndStatus` на прошлый `XdndPosition` — следующий не шлём, запоминаем последний.
    waiting_status: bool,
    pending_position: Option<(i16, i16, u32)>,
    /// `XdndDrop` отправлен: ждём `XdndFinished`, выделение ещё отдаём.
    pub(crate) dropped: bool,
}

impl PetsXdnd {
    /// Берёт `XdndSelection`; `None` — не вышло (сервер отдал выделение другому).
    pub(crate) fn start(
        xcb: &XCBConnection,
        atoms: &XcbAtoms,
        source: xproto::Window,
        uri_list: Vec<u8>,
    ) -> Option<Self> {
        if uri_list.is_empty() {
            return None;
        }
        xcb.set_selection_owner(source, atoms.XdndSelection, x11rb::CURRENT_TIME)
            .ok()?;
        let owner = xcb.get_selection_owner(atoms.XdndSelection).ok()?.reply().ok()?.owner;
        if owner != source {
            log::warn!("XDND: XdndSelection is owned by another window");
            return None;
        }
        // Список типов на окне — для получателей, которые читают его всегда, а не только при >3 типах.
        xcb.change_property32(
            PropMode::REPLACE,
            source,
            atoms.XdndTypeList,
            AtomEnum::ATOM,
            &[atoms.TextUriList],
        )
        .ok()?;
        xcb.flush().ok()?;
        log::debug!("XDND: drag started from window {source}");
        Some(Self {
            source,
            uri_list,
            target: None,
            waiting_status: false,
            pending_position: None,
            dropped: false,
        })
    }

    /// Указатель сдвинулся (координаты корня): сменить цель, если надо, и сообщить позицию.
    pub(crate) fn motion(
        &mut self,
        xcb: &XCBConnection,
        atoms: &XcbAtoms,
        root: xproto::Window,
        (x, y): (i16, i16),
        time: u32,
        is_own: impl Fn(xproto::Window) -> bool,
    ) {
        if self.dropped {
            return;
        }
        let found = find_aware(xcb, atoms, root, x, y).filter(|(window, _)| !is_own(*window));
        if found.map(|(window, _)| window) != self.target.map(|target| target.window) {
            log::debug!("XDND: target {found:?} at ({x}, {y})");
            if let Some(old) = self.target.take() {
                self.send(xcb, old.window, atoms.XdndLeave, [self.source, 0, 0, 0, 0]);
            }
            if let Some((window, version)) = found {
                let version = version.min(XDND_VERSION);
                self.send(
                    xcb,
                    window,
                    atoms.XdndEnter,
                    [self.source, version << 24, atoms.TextUriList, 0, 0],
                );
                self.target = Some(Target {
                    window,
                    version,
                    accepted: false,
                });
            }
            self.waiting_status = false;
            self.pending_position = None;
        }
        if self.target.is_some() {
            if self.waiting_status {
                self.pending_position = Some((x, y, time));
            } else {
                self.send_position(xcb, atoms, x, y, time);
            }
        }
        xcb.flush().ok();
    }

    /// `XdndStatus` от цели: принимает ли она бросок.
    pub(crate) fn status(&mut self, xcb: &XCBConnection, atoms: &XcbAtoms, data: [u32; 5]) {
        let Some(target) = self.target.as_mut().filter(|target| target.window == data[0]) else {
            return;
        };
        target.accepted = data[1] & 1 == 1;
        self.waiting_status = false;
        if let Some((x, y, time)) = self.pending_position.take() {
            self.send_position(xcb, atoms, x, y, time);
            xcb.flush().ok();
        }
    }

    /// Кнопку отпустили. `true` — бросок отправлен (ждём `XdndFinished`), `false` — сеанс окончен без броска.
    pub(crate) fn release(&mut self, xcb: &XCBConnection, atoms: &XcbAtoms, time: u32) -> bool {
        if self.dropped {
            return true;
        }
        let result = match self.target {
            Some(target) if target.accepted => {
                self.send(xcb, target.window, atoms.XdndDrop, [self.source, 0, time, 0, 0]);
                self.dropped = true;
                true
            }
            Some(target) => {
                self.send(xcb, target.window, atoms.XdndLeave, [self.source, 0, 0, 0, 0]);
                false
            }
            None => false,
        };
        xcb.flush().ok();
        result
    }

    /// Отмена (Esc): цель получает `XdndLeave`.
    pub(crate) fn cancel(&mut self, xcb: &XCBConnection, atoms: &XcbAtoms) {
        if let Some(target) = self.target.take()
            && !self.dropped
        {
            self.send(xcb, target.window, atoms.XdndLeave, [self.source, 0, 0, 0, 0]);
            xcb.flush().ok();
        }
    }

    /// `XdndFinished` пришёл от нашей цели.
    pub(crate) fn is_finished_by(&self, data: [u32; 5]) -> bool {
        self.target.is_some_and(|target| target.window == data[0])
    }

    /// Запрос `XdndSelection`: отдаём `text/uri-list`, остальное — отказ (`property = NONE`).
    pub(crate) fn selection_request(&self, xcb: &XCBConnection, atoms: &XcbAtoms, request: &SelectionRequestEvent) {
        let property = if request.property == x11rb::NONE {
            request.target
        } else {
            request.property
        };
        let served = request.target == atoms.TextUriList
            && xcb
                .change_property8(
                    PropMode::REPLACE,
                    request.requestor,
                    property,
                    atoms.TextUriList,
                    &self.uri_list,
                )
                .is_ok();
        let notify = SelectionNotifyEvent {
            response_type: xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: if served { property } else { x11rb::NONE },
        };
        if let Err(error) = xcb.send_event(false, request.requestor, EventMask::NO_EVENT, notify) {
            log::warn!("XDND: failed to answer selection request: {error}");
        }
        xcb.flush().ok();
    }

    fn send_position(&mut self, xcb: &XCBConnection, atoms: &XcbAtoms, x: i16, y: i16, time: u32) {
        let Some(target) = self.target else {
            return;
        };
        let packed = ((x as u16 as u32) << 16) | (y as u16 as u32);
        let time = if target.version >= 1 { time } else { 0 };
        self.send(
            xcb,
            target.window,
            atoms.XdndPosition,
            [self.source, 0, packed, time, atoms.XdndActionCopy],
        );
        self.waiting_status = true;
    }

    fn send(&self, xcb: &XCBConnection, window: xproto::Window, type_: u32, data: [u32; 5]) {
        let message = ClientMessageEvent::new(32, window, type_, data);
        if let Err(error) = xcb.send_event(false, window, EventMask::NO_EVENT, message) {
            log::warn!("XDND: failed to send client message: {error}");
        }
    }
}

/// Окно с `XdndAware` под точкой корня и его версия XDND: спуск от корня по детям, содержащим точку.
fn find_aware(
    xcb: &XCBConnection,
    atoms: &XcbAtoms,
    root: xproto::Window,
    x: i16,
    y: i16,
) -> Option<(xproto::Window, u32)> {
    let mut parent = root;
    for _ in 0..MAX_DEPTH {
        let child = xcb.translate_coordinates(root, parent, x, y).ok()?.reply().ok()?.child;
        if child == x11rb::NONE {
            return None;
        }
        if let Some(version) = aware_version(xcb, atoms, child) {
            return Some((child, version));
        }
        parent = child;
    }
    None
}

fn aware_version(xcb: &XCBConnection, atoms: &XcbAtoms, window: xproto::Window) -> Option<u32> {
    // Тип — любой: gpui ставит `XdndAware` с типом-атомом «XA_ATOM», а не предопределённым `ATOM`.
    let reply = xcb
        .get_property(false, window, atoms.XdndAware, AtomEnum::ANY, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next()
}
