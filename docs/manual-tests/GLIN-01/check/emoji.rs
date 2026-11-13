use gpui_kit::*;
struct V;
impl Render for V {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0xffffff)).text_color(rgb(0x000000)).text_size(px(32.))
            .flex().flex_col().p_4()
            .child(div().child("default: 😀 🎉 👍🏽 ❤️ 👨‍👩‍👧"))
            .child(div().font_family(".SystemUIFont").child("sysui: 😀 🎉 👍🏽 ❤️ 👨‍👩‍👧"))
            .child(div().font_family("Segoe UI").child("segoe: 😀 🎉 👍🏽 ❤️ 👨‍👩‍👧"))
            .child(div().font_family("DejaVu Sans").child("dejavu: 😀 🎉 👍🏽 ❤️ 👨‍👩‍👧"))
            .child(div().font_family("Noto Color Emoji").child("noto: 😀 🎉 👍🏽 ❤️ 👨‍👩‍👧"))
    }
}
fn main() {
    application().run(|cx: &mut App| {
        init(cx);
        let b = Bounds::new(point(px(0.), px(0.)), size(px(700.), px(330.)));
        let _ = open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(b)), ..Default::default() }, cx, |_, cx| cx.new(|_| V));
    });
}
