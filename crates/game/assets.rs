use macroquad::audio::{Sound, load_sound};
use macroquad::prelude::*;

/// File names match the original repo's Assets/ folder (GitHub Pages is case-sensitive).
pub struct Assets {
    pub font: Font,
    pub bg: Texture2D,
    /// Indexed by Dir::index(): Up, Down, Left, Right.
    pub cat: Vec<Texture2D>,
    pub rat: Vec<Texture2D>,
    pub rat_dead: Texture2D,
    pub tomato: Texture2D,
    pub boxx: Texture2D,
    pub hole: Texture2D,
    pub spike: Texture2D,
    pub spike_active: Texture2D,
    pub toolbar: Texture2D,
    pub toolbar_hover: Texture2D,
    pub music: Option<Sound>,
    pub countdown: Option<Sound>,
}

async fn tex(name: &str) -> Texture2D {
    let path = format!("Assets/{name}");
    load_texture(&path)
        .await
        .unwrap_or_else(|e| panic!("cannot load {path}: {e:?}"))
}

fn brighten_image(image: &mut Image) {
    for pixel in image.get_image_data_mut() {
        for channel in &mut pixel[..3] {
            *channel = (*channel).saturating_add(64);
        }
    }
}

async fn toolbar_textures() -> (Texture2D, Texture2D) {
    let image = load_image("Assets/UI_Toolbar.png")
        .await
        .expect("cannot load Assets/UI_Toolbar.png");
    let toolbar = Texture2D::from_image(&image);
    let mut brightened = image;
    brighten_image(&mut brightened);
    let toolbar_hover = Texture2D::from_image(&brightened);
    (toolbar, toolbar_hover)
}

impl Assets {
    pub async fn load() -> Assets {
        let font = load_ttf_font("Assets/font.ttf")
            .await
            .expect("cannot load Assets/font.ttf");

        let mut cat = Vec::new();
        let mut rat = Vec::new();
        for dir in ["Up", "Down", "Left", "Right"] {
            cat.push(tex(&format!("Cat_{dir}.png")).await);
            rat.push(tex(&format!("Mouse_{dir}.png")).await);
        }
        let (toolbar, toolbar_hover) = toolbar_textures().await;

        Assets {
            font,
            bg: tex("Bg.png").await,
            cat,
            rat,
            rat_dead: tex("Mouse_Dead.png").await,
            tomato: tex("Tomato.png").await,
            boxx: tex("Box.png").await,
            hole: tex("Hole.png").await,
            spike: tex("Spikes.png").await,
            spike_active: tex("Spikes_Active.png").await,
            toolbar,
            toolbar_hover,
            // Missing audio just means silence.
            music: load_sound("Assets/Music.ogg").await.ok(),
            countdown: load_sound("Assets/countdown.ogg").await.ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightening_lightens_rgb_and_preserves_alpha() {
        let mut image = Image {
            bytes: vec![80, 120, 250, 128],
            width: 1,
            height: 1,
        };

        brighten_image(&mut image);

        assert_eq!(image.get_image_data()[0], [144, 184, 255, 128]);
    }
}
