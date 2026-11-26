use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use macroquad::audio::{Sound, load_sound};
use macroquad::prelude::*;

/// Startup textures. Downloaded together so one slow file does not delay the
/// rest; gameplay is ready as soon as this batch is done.
const IMAGE_NAMES: [&str; 16] = [
    "Bg.png",
    "Cat_Up.png",
    "Cat_Down.png",
    "Cat_Left.png",
    "Cat_Right.png",
    "Mouse_Up.png",
    "Mouse_Down.png",
    "Mouse_Left.png",
    "Mouse_Right.png",
    "Mouse_Dead.png",
    "Tomato.png",
    "Box.png",
    "Hole.png",
    "Spikes.png",
    "Spikes_Active.png",
    "UI_Toolbar.png",
];

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
    pub countdown: Option<Sound>,
}

/// Poll every future each frame. Macroquad's executor is frame-driven and has
/// no join combinator; polling here starts all loads in the same frame.
async fn join_all<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut futures: Vec<Pin<Box<F>>> =
        futures.into_iter().map(Box::pin).collect();
    let mut outputs: Vec<Option<F::Output>> =
        (0..futures.len()).map(|_| None).collect();
    std::future::poll_fn(|cx: &mut Context<'_>| {
        let mut pending = false;
        for (index, future) in futures.iter_mut().enumerate() {
            if outputs[index].is_some() {
                continue;
            }
            match future.as_mut().poll(cx) {
                Poll::Ready(value) => outputs[index] = Some(value),
                Poll::Pending => pending = true,
            }
        }
        if pending {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
    outputs
        .into_iter()
        .map(|value| value.expect("joined future has a value"))
        .collect()
}

async fn image(name: &str) -> Image {
    let path = format!("Assets/{name}");
    load_image(&path)
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

fn toolbar_textures(image: Image) -> (Texture2D, Texture2D) {
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

        let futures = IMAGE_NAMES.iter().map(|name| image(name)).collect();
        let images = join_all(futures).await;
        let images: [Image; IMAGE_NAMES.len()] = match images.try_into() {
            Ok(images) => images,
            Err(_) => panic!("one image per texture"),
        };
        let [
            bg,
            cat_up,
            cat_down,
            cat_left,
            cat_right,
            rat_up,
            rat_down,
            rat_left,
            rat_right,
            rat_dead,
            tomato,
            boxx,
            hole,
            spike,
            spike_active,
            toolbar_image,
        ] = images;
        let (toolbar, toolbar_hover) = toolbar_textures(toolbar_image);

        Assets {
            font,
            bg: Texture2D::from_image(&bg),
            cat: [cat_up, cat_down, cat_left, cat_right]
                .map(|image| Texture2D::from_image(&image))
                .to_vec(),
            rat: [rat_up, rat_down, rat_left, rat_right]
                .map(|image| Texture2D::from_image(&image))
                .to_vec(),
            rat_dead: Texture2D::from_image(&rat_dead),
            tomato: Texture2D::from_image(&tomato),
            boxx: Texture2D::from_image(&boxx),
            hole: Texture2D::from_image(&hole),
            spike: Texture2D::from_image(&spike),
            spike_active: Texture2D::from_image(&spike_active),
            toolbar,
            toolbar_hover,
            // Music is large, so App loads it after the first frame.
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
