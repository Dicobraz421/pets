use gpui_kit::*;
use std::path::PathBuf;
struct Src;
struct Ghost;
impl Render for Ghost {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement { div().size(px(20.)).bg(rgb(0x3366ff)) }
}
#[derive(Clone)]
struct File(PathBuf);
impl Render for Src {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0xffffff)).child(
            div().id("f").size(px(100.)).bg(rgb(0xff8800)).child("drag me")
                .on_drag(File(PathBuf::from("/tmp/glin-01-payload.txt")), |_, _, _, cx| { eprintln!("DRAG START"); cx.new(|_| Ghost) })
                .external_drag_payload::<File>(|f, _, _| { eprintln!("PAYLOAD"); Some(ExternalDragPayload::Files(FileDragPaths::new([(f.0.clone(), false)]))) }),
        )
    }
}
struct Dst;
impl Render for Dst {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().id("dst").size_full().bg(rgb(0x88ff88)).child("drop here")
            .on_drop(|paths: &ExternalPaths, _, _| { println!("DROPPED {:?}", paths.paths()); })
    }
}
fn main() {
    env_logger::init();
    let dst = std::env::args().nth(1).as_deref() == Some("dst");
    application().run(move |cx: &mut App| {
        init(cx);
        let x = if dst { 400. } else { 0. };
        let b = Bounds::new(point(px(x), px(0.)), size(px(300.), px(300.)));
        let o = WindowOptions { window_bounds: Some(WindowBounds::Windowed(b)), ..Default::default() };
        if dst { let _ = open_window(o, cx, |_, cx| cx.new(|_| Dst)); } else { let _ = open_window(o, cx, |_, cx| cx.new(|_| Src)); }
    });
}
