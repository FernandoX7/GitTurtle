use gpui_kit::component::Root;
use gpui_kit::*;

struct GitTurtle;

impl Render for GitTurtle {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x0f171c)).text_color(rgb(0xdee9ed)).child("GitTurtle")
    }
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|_| GitTurtle);
                cx.new(|cx| Root::new(view, window, cx))
            }).expect("open GitTurtle window");
        }).detach();
    });
}
