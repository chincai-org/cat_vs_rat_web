use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use macroquad::audio::Sound;
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

/// A texture plus where its visible art sits, so it can be centred in a cell
/// even though the source images have transparent padding around the art.
#[derive(Clone)]
pub struct Art {
    pub texture: Texture2D,
    /// Centre of the visible art as a fraction of the texture size.
    pub center: Vec2,
}

/// File names match the original repo's Assets/ folder (GitHub Pages is case-sensitive).
pub struct Assets {
    pub font: Font,
    pub bg: Texture2D,
    /// Indexed by Dir::index(): Up, Down, Left, Right.
    pub cat: Vec<Art>,
    pub rat: Vec<Art>,
    pub rat_dead: Art,
    pub tomato: Art,
    pub boxx: Art,
    pub hole: Art,
    pub spike: Art,
    pub spike_active: Art,
    pub toolbar: Texture2D,
    pub toolbar_hover: Texture2D,
    pub countdown: Option<Sound>,
}

/// Centre of the widest opaque run in every row and column, weighted by run
/// length. Character art has a tail sticking out to one side; a plain bounding
/// box would pull the body off-centre, while the widest runs stay on the body.
fn body_center(image: &Image) -> Vec2 {
    let w = image.width as usize;
    let h = image.height as usize;
    let pixels = image.get_image_data();
    let opaque = |x: usize, y: usize| pixels[y * w + x][3] > 127;

    let mut sx = 0.0;
    let mut sw = 0.0;
    for y in 0..h {
        if let Some((start, end)) = longest_run((0..w).map(|x| opaque(x, y)), w)
        {
            let len = (end - start + 1) as f32;
            sx += (start + end) as f32 * 0.5 * len;
            sw += len;
        }
    }
    let cx = if sw > 0.0 { sx / sw } else { w as f32 * 0.5 };

    let mut sy = 0.0;
    let mut sh = 0.0;
    for x in 0..w {
        if let Some((start, end)) = longest_run((0..h).map(|y| opaque(x, y)), h)
        {
            let len = (end - start + 1) as f32;
            sy += (start + end) as f32 * 0.5 * len;
            sh += len;
        }
    }
    let cy = if sh > 0.0 { sy / sh } else { h as f32 * 0.5 };

    vec2(cx / w as f32, cy / h as f32)
}

/// Centre of the full opaque bounding box, including shadows.
fn bbox_center(image: &Image) -> Vec2 {
    let w = image.width as usize;
    let h = image.height as usize;
    let pixels = image.get_image_data();
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, 0, 0);
    let mut any = false;
    for y in 0..h {
        for x in 0..w {
            if pixels[y * w + x][3] > 0 {
                any = true;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    if any {
        vec2(
            (min_x + max_x + 1) as f32 / 2.0 / w as f32,
            (min_y + max_y + 1) as f32 / 2.0 / h as f32,
        )
    } else {
        vec2(0.5, 0.5)
    }
}

fn longest_run(
    bits: impl Iterator<Item = bool>,
    len: usize,
) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    let mut start = None;
    for (index, bit) in bits.enumerate() {
        if bit {
            start.get_or_insert(index);
        } else if let Some(first) = start.take() {
            best = wider(best, (first, index - 1));
        }
    }
    if let Some(first) = start {
        best = wider(best, (first, len - 1));
    }
    best
}

fn wider(
    best: Option<(usize, usize)>,
    run: (usize, usize),
) -> Option<(usize, usize)> {
    match best {
        Some(current) if current.1 - current.0 >= run.1 - run.0 => {
            Some(current)
        }
        _ => Some(run),
    }
}

fn actor_art(image: &Image) -> Art {
    Art {
        texture: Texture2D::from_image(image),
        center: body_center(image),
    }
}

fn object_art(image: &Image) -> Art {
    Art {
        texture: Texture2D::from_image(image),
        center: bbox_center(image),
    }
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

/// Join two futures with different output types. `join_all` only handles one
/// future type; this starts the font in the same frame as textures.
async fn join_pair<A, B>(
    first: impl Future<Output = A>,
    second: impl Future<Output = B>,
) -> (A, B) {
    let mut first = Box::pin(first);
    let mut second = Box::pin(second);
    let mut first_output = None;
    let mut second_output = None;
    std::future::poll_fn(|cx: &mut Context<'_>| {
        if first_output.is_none() {
            if let Poll::Ready(value) = first.as_mut().poll(cx) {
                first_output = Some(value);
            }
        }
        if second_output.is_none() {
            if let Poll::Ready(value) = second.as_mut().poll(cx) {
                second_output = Some(value);
            }
        }
        if first_output.is_some() && second_output.is_some() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    (
        first_output.expect("first joined future has a value"),
        second_output.expect("second joined future has a value"),
    )
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
        let font = load_ttf_font("Assets/font.ttf");
        let futures = IMAGE_NAMES.iter().map(|name| image(name)).collect();
        let (font, images) = join_pair(font, join_all(futures)).await;
        let font = font.expect("cannot load Assets/font.ttf");
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

        let spike = object_art(&spike);
        let mut spike_active = object_art(&spike_active);
        // The raised frame grows upward from the same base, so it shares the
        // inactive frame's centre instead of jumping when the spikes toggle.
        spike_active.center = spike.center;

        Assets {
            font,
            bg: Texture2D::from_image(&bg),
            cat: [&cat_up, &cat_down, &cat_left, &cat_right]
                .map(actor_art)
                .to_vec(),
            rat: [&rat_up, &rat_down, &rat_left, &rat_right]
                .map(actor_art)
                .to_vec(),
            rat_dead: actor_art(&rat_dead),
            tomato: object_art(&tomato),
            boxx: object_art(&boxx),
            hole: object_art(&hole),
            spike,
            spike_active,
            toolbar,
            toolbar_hover,
            // App loads the countdown sound after the first frame.
            countdown: None,
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

    /// Opaque pixels at the given coordinates on an otherwise empty image.
    fn image_from(width: u16, height: u16, opaque: &[(u16, u16)]) -> Image {
        let mut bytes = vec![0; width as usize * height as usize * 4];
        for &(x, y) in opaque {
            let index = (y as usize * width as usize + x as usize) * 4;
            bytes[index..index + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
        Image {
            bytes,
            width,
            height,
        }
    }

    #[test]
    fn bbox_center_uses_the_visible_corners() {
        let image = image_from(8, 8, &[(0, 0), (1, 0)]);

        let center = bbox_center(&image);

        assert!((center.x - 0.125).abs() < 1e-6, "x={}", center.x);
        assert!((center.y - 0.0625).abs() < 1e-6, "y={}", center.y);
    }

    #[test]
    fn empty_art_is_treated_as_centred() {
        assert_eq!(bbox_center(&image_from(4, 4, &[])), vec2(0.5, 0.5));
        assert_eq!(body_center(&image_from(4, 4, &[])), vec2(0.5, 0.5));
    }

    #[test]
    fn body_center_ignores_a_thin_tail() {
        // A 4x4 body at x=2..5 with a one-pixel tail sticking out to the right.
        let mut opaque = Vec::new();
        for y in 2..=5 {
            for x in 2..=5 {
                opaque.push((x, y));
            }
        }
        opaque.push((7, 4));
        let image = image_from(8, 8, &opaque);

        let body = body_center(&image);
        let bbox = bbox_center(&image);

        assert!((body.x - 3.5 / 8.0).abs() < 1e-6, "body x={}", body.x);
        assert!(bbox.x > body.x, "the tail should pull the bbox right");
    }
}
