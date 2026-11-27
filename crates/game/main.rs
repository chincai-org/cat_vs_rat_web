mod app;
mod assets;
mod editor_ui;
mod keymap;
mod store;
mod view;

use macroquad::prelude::*;

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn cat_vs_rat_ready();
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Cat vs Rat".to_owned(),
        window_width: 1280,
        window_height: 640,
        high_dpi: true,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let assets = assets::Assets::load().await;
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: The JavaScript import only hides the HTML loading overlay.
        unsafe { cat_vs_rat_ready() };
    }
    let mut app = app::App::new(assets);
    loop {
        clear_background(BLACK);
        let view = view::View::new();
        app.frame(&view);
        next_frame().await;
    }
}
