//! Значок в области уведомлений (трей): значок из ресурса exe, подсказка, щелчок и меню по правой кнопке.
//!
//! Значку нужно своё окно — скрытое, в потоке интерфейса: его сообщения разбирает цикл сообщений gpui. Проводник
//! перезапустился («TaskbarCreated») — значок ставится заново. Обработчик событий вызывается из оконной процедуры
//! этого окна: в нём — только передать событие дальше (канал в задачу интерфейса), не трогать gpui и не удалять
//! [`Tray`].

use std::cell::{Cell, RefCell};
use std::io;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFY_ICON_MESSAGE, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GWLP_USERDATA,
    GetCursorPos, GetSystemMetrics, GetWindowLongPtrW, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LR_SHARED, LoadImageW,
    MF_SEPARATOR, MF_STRING, PostMessageW, RegisterClassW, RegisterWindowMessageW, SM_CXSMICON, SM_CYSMICON,
    SetForegroundWindow, SetWindowLongPtrW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};
use windows::core::{PCWSTR, w};

use super::com_error;

/// Сообщение значка окну.
const CALLBACK: u32 = WM_APP + 1;
/// Номер значка у окна (значок один).
const ID: u32 = 1;
/// Подсказка — не длиннее поля `szTip` (с нулём в конце).
const TIP: usize = 128;

/// Событие значка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Щелчок левой кнопкой.
    Click,
    /// Выбран пункт меню — его номер.
    Menu(u32),
}

/// Пункт меню значка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// Номер (больше нуля) и текст.
    Command(u32, String),
    Separator,
}

struct State {
    hwnd: Cell<HWND>,
    icon: Cell<HICON>,
    tip: RefCell<String>,
    menu: RefCell<Vec<Item>>,
    taskbar_created: u32,
    on_event: Box<dyn Fn(Event)>,
}

/// Значок в трее: живёт, пока жив; создавать и удалять — в потоке интерфейса.
pub struct Tray {
    state: *mut State,
}

impl Tray {
    /// Поставить значок: `icon` — номер группы значков в ресурсах exe, `tip` — подсказка.
    pub fn new(icon: u16, tip: &str, on_event: impl Fn(Event) + 'static) -> io::Result<Self> {
        let class = w!("PetsTray");
        // SAFETY: null — модуль exe процесса.
        let module = unsafe { GetModuleHandleW(None) }.map_err(|e| com_error("GetModuleHandleW", e))?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        };
        // SAFETY: структура и строка класса живы на время вызова. Класс уже есть (второй значок) — не ошибка:
        // настоящую покажет `CreateWindowExW`.
        unsafe { RegisterClassW(&wc) };
        // SAFETY: строки — статические; окно без родителя и без стиля — скрытое, сообщения — в этом потоке.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                w!(""),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(module.into()),
                None,
            )
        }
        .map_err(|e| com_error("CreateWindowExW tray", e))?;
        let state = Box::new(State {
            hwnd: Cell::new(hwnd),
            icon: Cell::new(HICON::default()),
            tip: RefCell::new(tip.to_owned()),
            menu: RefCell::new(Vec::new()),
            // SAFETY: строка — статическая.
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            on_event: Box::new(on_event),
        });
        let tray = Self {
            state: Box::into_raw(state),
        };
        // SAFETY: окно наше; указатель жив до `Drop`, где сначала обнуляется.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, tray.state as isize) };
        let state = tray.state();
        state.icon.set(load_icon(icon)?);
        state.notify(NIM_ADD)?;
        Ok(tray)
    }

    fn state(&self) -> &State {
        // SAFETY: указатель из `Box::into_raw`, освобождается только в `Drop`.
        unsafe { &*self.state }
    }

    /// Подсказка при наведении.
    pub fn set_tip(&self, tip: &str) -> io::Result<()> {
        let state = self.state();
        if *state.tip.borrow() == tip {
            return Ok(());
        }
        tip.clone_into(&mut state.tip.borrow_mut());
        state.notify(NIM_MODIFY)
    }

    /// Другой значок из ресурсов exe (например, с отметкой «есть непрочитанные»).
    pub fn set_icon(&self, icon: u16) -> io::Result<()> {
        let state = self.state();
        state.icon.set(load_icon(icon)?);
        state.notify(NIM_MODIFY)
    }

    /// Меню по правой кнопке; пустое — меню нет.
    pub fn set_menu(&self, items: Vec<Item>) {
        *self.state().menu.borrow_mut() = items;
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let state = self.state();
        let hwnd = state.hwnd.get();
        let _ = state.notify(NIM_DELETE);
        // SAFETY: окно наше и ещё живо; после обнуления оконная процедура состояние не читает; `Box` освобождается
        // один раз.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(hwnd);
            drop(Box::from_raw(self.state));
        }
    }
}

impl State {
    fn notify(&self, message: NOTIFY_ICON_MESSAGE) -> io::Result<()> {
        let data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd.get(),
            uID: ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: CALLBACK,
            hIcon: self.icon.get(),
            szTip: tip_buf(&self.tip.borrow()),
            ..Default::default()
        };
        // SAFETY: структура жива на время вызова, размер указан.
        if unsafe { Shell_NotifyIconW(message, &data) }.as_bool() {
            Ok(())
        } else {
            Err(io::Error::other(format!("Shell_NotifyIconW {}", message.0)))
        }
    }

    /// Меню у курсора; выбранный пункт — событием.
    fn show_menu(&self) {
        let items = self.menu.borrow().clone();
        if items.is_empty() {
            return;
        }
        let hwnd = self.hwnd.get();
        // SAFETY: меню создаётся и уничтожается здесь же; строки пунктов живы на время `AppendMenuW` (меню их
        // копирует); окно наше. `SetForegroundWindow` и пустое сообщение после — чтобы меню закрывалось щелчком мимо.
        let chosen = unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            for item in &items {
                let _ = match item {
                    Item::Command(id, label) => {
                        let label = wide(label);
                        AppendMenuW(menu, MF_STRING, *id as usize, PCWSTR(label.as_ptr()))
                    }
                    Item::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()),
                };
            }
            let mut at = POINT::default();
            let _ = GetCursorPos(&mut at);
            let _ = SetForegroundWindow(hwnd);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                at.x,
                at.y,
                Some(0),
                hwnd,
                None,
            );
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            chosen.0
        };
        if chosen > 0 {
            (self.on_event)(Event::Menu(chosen as u32));
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: в данных окна — ноль или указатель на `State`, живой до обнуления в `Tray::drop` (тот же поток).
    let state = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref() };
    if let Some(state) = state {
        if msg == CALLBACK {
            match lparam.0 as u32 {
                WM_LBUTTONUP => (state.on_event)(Event::Click),
                WM_RBUTTONUP | WM_CONTEXTMENU => state.show_menu(),
                _ => {}
            }
            return LRESULT(0);
        }
        if msg == state.taskbar_created {
            // Проводник перезапустился: прежнего значка нет.
            let _ = state.notify(NIM_ADD);
            return LRESULT(0);
        }
    }
    // SAFETY: параметры — как пришли.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Малый значок группы `resource` из exe; общий (`LR_SHARED`) — освобождать не нужно.
fn load_icon(resource: u16) -> io::Result<HICON> {
    // SAFETY: номер ресурса — целым в PCWSTR (MAKEINTRESOURCE); модуль exe жив, пока жив процесс.
    let handle = unsafe {
        let module = GetModuleHandleW(None).map_err(|e| com_error("GetModuleHandleW", e))?;
        LoadImageW(
            Some(module.into()),
            PCWSTR(resource as usize as *const u16),
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_DEFAULTCOLOR | LR_SHARED,
        )
    }
    .map_err(|e| com_error(&format!("LoadImageW icon {resource}"), e))?;
    Ok(HICON(handle.0))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Подсказка в поле `szTip`: обрезается по целому символу, в конце — ноль.
fn tip_buf(tip: &str) -> [u16; TIP] {
    let mut buf = [0u16; TIP];
    let mut len = 0;
    for ch in tip.chars() {
        let mut units = [0u16; 2];
        let units = ch.encode_utf16(&mut units);
        if len + units.len() > TIP - 1 {
            break;
        }
        buf[len..len + units.len()].copy_from_slice(units);
        len += units.len();
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::{TIP, tip_buf};

    /// Подсказка влезает целиком; длинная — обрезана по целому символу, ноль в конце остаётся.
    #[test]
    fn pets_tray_tip_fits() {
        let short = tip_buf("Postman — 3");
        assert_eq!(String::from_utf16_lossy(&short[..11]), "Postman — 3");
        assert_eq!(short[11], 0);
        let long = tip_buf(&"😀".repeat(100));
        assert_eq!(long[TIP - 1], 0);
        assert_eq!(
            long.iter().filter(|&&u| u != 0).count(),
            TIP - 2,
            "63 пары, непарной половины нет"
        );
    }
}
