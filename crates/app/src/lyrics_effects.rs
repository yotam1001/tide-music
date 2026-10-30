//! View-owned album backdrop. No URL fetching or global image cache.
//! Blur a small bitmap once per cover, rather than blurring the window per frame.
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

/// Drop the UI's owners as well as the controller's owners. An empty image
/// removes the software renderer's source; the page is conditionally destroyed.
pub fn release_ui(ui: &crate::AppWindow) {
    ui.set_lyrics_lines(slint::ModelRc::new(
        slint::VecModel::<crate::LyricRow>::default(),
    ));
    ui.set_lyrics_active(-1);
    ui.set_lyrics_synced(false);
    ui.set_lyrics_source("".into());
    ui.set_lyrics_backdrop(Image::default());
    ui.set_lyrics_effects_active(false);
}

pub const BACKDROP_SIDE: u32 = 64;
pub const BACKDROP_BYTES: usize = (BACKDROP_SIDE * BACKDROP_SIDE * 4) as usize;

#[derive(Default)]
pub struct Backdrop {
    source: Option<Image>,
    image: Image,
}
impl Backdrop {
    pub fn update(&mut self, source: Image) -> Image {
        if self.source.as_ref() != Some(&source) {
            self.image = make_backdrop(&source);
            self.source = Some(source);
        }
        self.image.clone()
    }

    pub fn clear(&mut self) {
        // Replace, rather than clear a retained pixel vector. The ordinary
        // player may still share the original cover; this view keeps no clone.
        *self = Self::default();
    }

    pub fn allocated_bytes(&self) -> usize {
        (self.image.size().width * self.image.size().height * 4) as usize
    }
}

fn make_backdrop(source: &Image) -> Image {
    let size = source.size();
    // Ordinary artwork is already a bounded 96px thumbnail. Reject unexpectedly
    // large inputs before requesting a CPU pixel conversion.
    if size.width == 0 || size.height == 0 || size.width > 512 || size.height > 512 {
        return Image::default();
    }
    let Some(pixels) = source.to_rgba8() else {
        return Image::default();
    };
    let Some(original) =
        image::RgbaImage::from_raw(size.width, size.height, pixels.as_bytes().to_vec())
    else {
        return Image::default();
    };
    let small = image::imageops::resize(
        &original,
        BACKDROP_SIDE,
        BACKDROP_SIDE,
        image::imageops::FilterType::Triangle,
    );
    let blurred = image::imageops::blur(&small, 8.0);
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        blurred.as_raw(),
        BACKDROP_SIDE,
        BACKDROP_SIDE,
    ))
}
