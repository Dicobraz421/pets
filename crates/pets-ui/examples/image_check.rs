//! Проверка картинок Windows-платформы gpui (`pets-gpui-windows`): кадр больше 1024 px, кадр, обрезанный краем
//! прокрутки, и ловушка раскладки `img` (Postman S5-07). `cargo run -p pets-ui --example image_check -- <каталог>` —
//! окно без фокуса, снимок клиентской области `PrintWindow` в `<каталог>/image-check.png`, сверка пикселей в stdout;
//! код выхода 0 — всё сошлось.
//!
//! Кадр — «координатный» градиент: в пикселе (x, y) R = x % 256, G = y % 256, B = (x / 256) · 16 + y / 256,
//! так что по снятому цвету видно, какой пиксель кадра попал на экран (сдвиг — в пикселях кадра).
//! Синтетического ввода нет: прокрутка задаётся `ScrollHandle::set_offset` до первого кадра.
//!
//! Панели: A — кадр 2000×1400, вписанный в 600×300; B — кадр 600×400 в прокрутке, обрезан верхним краем на 100 px;
//! C — кадр 2000×1400 в своём размере в прокрутке, сдвинут на 500 px; D — ловушка: страница 1060×1500 как
//! `img().w_full()` в колонке 300 px (высота элемента остаётся 1500, `ObjectFit::Contain` ставит картинку
//! по центру этой высоты — в панели пусто); E — то же через обёртку с `aspect_ratio` (картинка на месте).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::*;

/// Большой кадр — как картинка письма до 1600 px.
const BIG: (u32, u32) = (2000, 1400);
/// Малый кадр (≤ 1024) — для обрезки краем прокрутки.
const SMALL: (u32, u32) = (600, 400);
/// Страница PDF (A4 при 1500 px по высоте).
const PAGE: (u32, u32) = (1060, 1500);
/// Панель A–C, логические px.
const PANE: (f32, f32) = (600., 300.);
/// Узкая панель D–E (колонка письма).
const NARROW: f32 = 300.;
/// Фон панели: «пусто» на сверке.
const PANE_BG: u32 = 0x202020;
/// Сколько кадров показать до снимка.
const FRAMES: u32 = 6;

/// Кадр gpui (BGRA) с координатным градиентом.
fn gradient(w: u32, h: u32) -> Option<Arc<RenderImage>> {
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            bgra.extend_from_slice(&[((x / 256) * 16 + y / 256) as u8, (y % 256) as u8, (x % 256) as u8, 255]);
        }
    }
    let buffer = image::RgbaImage::from_raw(w, h, bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// Пиксель кадра, который показывает снятый цвет (обратное к [`gradient`]).
fn decode(p: [u8; 4]) -> (i32, i32) {
    let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
    ((b >> 4) * 256 + r, (b & 15) * 256 + g)
}

/// Одна сверка: точка панели (логические px от её угла) и какой пиксель кадра там должен быть
/// (`None` — фон панели: картинки там нет).
struct Probe {
    name: &'static str,
    /// Панель: A–C — 0…2 (первый ряд), D–E — 3…4 (второй ряд).
    pane: usize,
    at: (f32, f32),
    want: Option<(i32, i32)>,
}

fn probes() -> Vec<Probe> {
    // A: ScaleDown по высоте — 2000×1400 → 428,6×300, по центру панели.
    let fit = PANE.1 / BIG.1 as f32;
    let left = (PANE.0 - BIG.0 as f32 * fit) / 2.;
    let a = |x: i32, y: i32| Probe {
        name: "A big fit",
        pane: 0,
        at: (left + x as f32 * fit, y as f32 * fit),
        want: Some((x, y)),
    };
    // E: страница шириной 300 — масштаб 300 / 1060.
    let page = NARROW / PAGE.0 as f32;
    let e = |x: i32, y: i32| Probe {
        name: "E aspect wrap",
        pane: 4,
        at: (x as f32 * page, y as f32 * page),
        want: Some((x, y)),
    };
    let p = |name, pane, at, want| Probe { name, pane, at, want };
    vec![
        a(300, 300),
        a(1500, 1000),
        a(1900, 1300),
        p("B small clipped", 1, (10., 10.), Some((10, 110))),
        p("B small clipped", 1, (300., 150.), Some((300, 250))),
        p("C big clipped", 2, (10., 10.), Some((10, 510))),
        p("C big clipped", 2, (500., 250.), Some((500, 750))),
        p("D w_full trap", 3, (150., 150.), None),
        e(530, 530),
        e(100, 1000),
    ]
}

struct Check {
    big: Arc<RenderImage>,
    small: Arc<RenderImage>,
    page: Arc<RenderImage>,
    scroll_b: ScrollHandle,
    scroll_c: ScrollHandle,
    frames: u32,
    out: PathBuf,
}

/// Панель: фон «пусто», содержимое обрезано по краю.
fn pane(w: f32, content: impl IntoElement) -> Div {
    div()
        .w(px(w))
        .h(px(PANE.1))
        .flex()
        .flex_none()
        .overflow_hidden()
        .bg(rgb(PANE_BG))
        .child(content)
}

/// Вертикальная прокрутка на всю панель.
fn scroll(id: &'static str, handle: &ScrollHandle) -> Stateful<Div> {
    div()
        .id(id)
        .size_full()
        .flex()
        .flex_col()
        .overflow_x_hidden()
        .overflow_y_scroll()
        .track_scroll(handle)
}

impl Render for Check {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.frames < FRAMES {
            self.frames += 1;
            window.request_animation_frame();
        } else if self.frames == FRAMES {
            self.frames += 1;
            capture(self.out.clone(), window.scale_factor(), window, cx);
        }
        let a = pane(
            PANE.0,
            img(self.big.clone())
                .w(px(PANE.0))
                .h(px(PANE.1))
                .object_fit(ObjectFit::ScaleDown),
        );
        let b = pane(
            PANE.0,
            scroll("b", &self.scroll_b)
                .child(div().h(px(200.)).flex_none())
                .child(
                    img(self.small.clone())
                        .w(px(SMALL.0 as f32))
                        .h(px(SMALL.1 as f32))
                        .flex_none(),
                )
                .child(div().h(px(400.)).flex_none()),
        );
        let c = pane(
            PANE.0,
            scroll("c", &self.scroll_c).child(
                img(self.big.clone())
                    .w(px(BIG.0 as f32))
                    .h(px(BIG.1 as f32))
                    .flex_none(),
            ),
        );
        // Как лист PDF в Postman до обхода: ширина колонки, высоту `img` берёт «свою» — 1500.
        let d = pane(
            NARROW,
            div()
                .id("d")
                .size_full()
                .overflow_y_scroll()
                .child(img(self.page.clone()).w_full()),
        );
        // Обход: высоту задаёт обёртка по пропорции кадра, у `img` — `size_full` (обе стороны заданы).
        let ratio = PAGE.0 as f32 / PAGE.1 as f32;
        let e = pane(
            NARROW,
            div().id("e").size_full().overflow_y_scroll().child(
                div()
                    .w_full()
                    .max_w(px(PAGE.0 as f32))
                    .aspect_ratio(ratio)
                    .child(img(self.page.clone()).size_full()),
            ),
        );
        let row = || div().flex().gap(px(20.));
        div()
            .size_full()
            .bg(rgb(0x000000))
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(row().child(a).child(b).child(c))
            .child(row().child(d).child(e))
    }
}

/// Левый верхний угол панели `n` в логических px окна (см. `render`: отступ 20, зазор 20).
fn pane_origin(n: usize) -> (f32, f32) {
    match n {
        0..=2 => (20. + n as f32 * (PANE.0 + 20.), 20.),
        n => (20. + (n - 3) as f32 * (NARROW + 20.), 40. + PANE.1),
    }
}

/// Снимок не из потока окна: `PrintWindow` шлёт окну сообщения (повторный вход в wndproc gpui).
fn capture(out: PathBuf, scale: f32, window: &Window, cx: &mut Context<Check>) {
    let hwnd = match raw_window_handle::HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
        Ok(raw_window_handle::RawWindowHandle::Win32(h)) => h.hwnd.get(),
        other => {
            eprintln!("no Win32 window handle: {other:?}");
            std::process::exit(2);
        }
    };
    let task = cx.background_spawn(async move { shoot::shoot(hwnd) });
    cx.spawn(async move |_, _| {
        let code = match task.await {
            Ok(shot) => report(&shot, scale, &out),
            Err(e) => {
                eprintln!("capture failed: {e}");
                2
            }
        };
        std::process::exit(code);
    })
    .detach();
}

/// Сохранить снимок и сверить пиксели; код выхода — 0, если всё сошлось (±1 px кадра в B и C, ±3 в A и E —
/// масштаб; плюс пиксель на округление координат).
fn report(shot: &image::RgbaImage, scale: f32, out: &Path) -> i32 {
    let path = out.join("image-check.png");
    if let Err(e) = shot.save(&path) {
        eprintln!("{}: {e}", path.display());
        return 2;
    }
    println!(
        "snapshot {} {}x{}, scale {scale}",
        path.display(),
        shot.width(),
        shot.height()
    );
    let mut failed = 0;
    for p in probes() {
        let (ox, oy) = pane_origin(p.pane);
        let (x, y) = (((ox + p.at.0) * scale) as u32, ((oy + p.at.1) * scale) as u32);
        let Some(px) = shot.get_pixel_checked(x, y) else {
            println!("{:16} at ({x},{y}): outside the snapshot", p.name);
            failed += 1;
            continue;
        };
        let bg = px.0[..3] == PANE_BG.to_be_bytes()[1..];
        let got = decode(px.0);
        let ok = match p.want {
            None => bg,
            Some(want) => {
                let tol = if p.pane == 1 || p.pane == 2 { 1 } else { 3 } + (1. / scale).ceil() as i32;
                !bg && px.0 != [0, 0, 0, 255] && (got.0 - want.0).abs() <= tol && (got.1 - want.1).abs() <= tol
            }
        };
        if !ok {
            failed += 1;
        }
        let want = p.want.map_or_else(|| "background".to_string(), |w| format!("{w:?}"));
        let got = if bg {
            "background".to_string()
        } else {
            format!("{got:?}")
        };
        println!(
            "{:16} want {want:12} got {got:12} rgb {:?} {}",
            p.name,
            &px.0[..3],
            if ok { "ok" } else { "FAIL" }
        );
    }
    i32::from(failed > 0)
}

fn main() {
    let out = std::env::args().nth(1).map_or_else(std::env::temp_dir, PathBuf::from);
    let (Some(big), Some(small), Some(page)) = (
        gradient(BIG.0, BIG.1),
        gradient(SMALL.0, SMALL.1),
        gradient(PAGE.0, PAGE.1),
    ) else {
        eprintln!("pixel buffer size mismatch");
        std::process::exit(2);
    };
    application().run(move |cx: &mut App| {
        init(cx);
        let bounds = Bounds::new(
            point(px(40.), px(40.)),
            size(px(20. + 3. * (PANE.0 + 20.)), px(2. * PANE.1 + 60.)),
        );
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            focus: false,
            ..Default::default()
        };
        let opened = open_window(options, cx, |_, cx| {
            let (scroll_b, scroll_c) = (ScrollHandle::new(), ScrollHandle::new());
            // B: распорка 200 + сдвиг 300 — кадр уходит за верхний край на 100 px; C — на 500 px.
            scroll_b.set_offset(point(px(0.), px(-300.)));
            scroll_c.set_offset(point(px(0.), px(-500.)));
            cx.new(|_| Check {
                big,
                small,
                page,
                scroll_b,
                scroll_c,
                frames: 0,
                out,
            })
        });
        if let Err(e) = opened {
            eprintln!("open window: {e}");
            std::process::exit(2);
        }
    });
}

/// `PrintWindow(PW_RENDERFULLCONTENT)` клиентской области (с выключенной DirectComposition снимает DirectX).
mod shoot {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GdiFlush, GetDC, ReleaseDC, SelectObject,
    };
    use windows_sys::Win32::Storage::Xps::{PW_CLIENTONLY, PrintWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, PW_RENDERFULLCONTENT};

    pub fn shoot(hwnd: isize) -> Result<image::RgbaImage, String> {
        let hwnd = hwnd as HWND;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `rect` — валидный RECT; неверное окно даёт ошибку, а не UB.
        if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
            return Err("GetClientRect".into());
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w <= 0 || h <= 0 {
            return Err(format!("empty client area {w}x{h}"));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: контексты и картинка создаются и освобождаются здесь же; PrintWindow — не из потока окна;
        // DIB 32 бит w×h без выравнивания строк — ровно w·h·4 байт, читаются до удаления картинки.
        unsafe {
            let screen = GetDC(hwnd);
            if screen.is_null() {
                return Err("GetDC".into());
            }
            let dc = CreateCompatibleDC(screen);
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            let result = if dc.is_null() || bitmap.is_null() || bits.is_null() {
                Err("CreateDIBSection".to_string())
            } else {
                let old = SelectObject(dc, bitmap);
                let printed = PrintWindow(hwnd, dc, PW_CLIENTONLY | PW_RENDERFULLCONTENT) != 0;
                GdiFlush();
                let bgra = std::slice::from_raw_parts(bits as *const u8, w as usize * h as usize * 4);
                let rgba = bgra
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [p[2], p[1], p[0], 255])
                    .collect();
                SelectObject(dc, old);
                if printed {
                    image::RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or_else(|| "buffer".to_string())
                } else {
                    Err("PrintWindow".into())
                }
            };
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !dc.is_null() {
                DeleteDC(dc);
            }
            ReleaseDC(hwnd, screen);
            result
        }
    }
}
