mod app;
mod assets;
mod editor_ui;
mod store;
mod view;

use macroquad::prelude::*;

fn window_conf() -> Conf {
    Conf {
        window_title: "Cat vs Rat".to_owned(),
        window_width: 1280,
        window_height: 640,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let assets = assets::Assets::load().await;
    let mut app = app::App::new(assets);
    loop {
        clear_background(BLACK);
        let view = view::View::new();
        app.frame(&view);
        next_frame().await;
    }
}
