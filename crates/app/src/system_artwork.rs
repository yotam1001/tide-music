//! Reuse the current bounded cover for Windows media controls, once per change.
use slint::Image;

#[derive(Default)]
pub(super) struct Publisher {
    last: Option<(String, String, u32, u32)>,
}
impl Publisher {
    pub fn reset(&mut self) {
        self.last = None;
    }
    pub fn update(
        &mut self,
        session: &mut tide_windows::MediaSession,
        source: &str,
        artwork_url: &str,
        image: &Image,
    ) {
        let size = image.size();
        if self
            .last
            .as_ref()
            .is_some_and(|(old_source, old_url, width, height)| {
                old_source == source
                    && old_url == artwork_url
                    && *width == size.width
                    && *height == size.height
            })
        {
            return;
        }
        // Avoid a failing OS integration retrying encoding on every UI timer tick.
        self.last = Some((
            source.to_owned(),
            artwork_url.to_owned(),
            size.width,
            size.height,
        ));
        let png = encode(image);
        let _ = session.set_thumbnail(png.as_deref());
    }
}
fn encode(image: &Image) -> Option<Vec<u8>> {
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
    let size = image.size();
    // Only publish Tide's decoded native thumbnail, never source-resolution images.
    if size.width == 0 || size.height == 0 || size.width > 160 || size.height > 160 {
        return None;
    }
    let pixels = image.to_rgb8()?;
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(
            pixels.as_bytes(),
            size.width,
            size.height,
            ExtendedColorType::Rgb8,
        )
        .ok()?;
    (bytes.len() <= 128 * 1024).then_some(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_cover_roundtrips_only_small_decoded_images() {
        let pixels = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(160, 160);
        let png = encode(&Image::from_rgb8(pixels)).unwrap();
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (160, 160));
        assert!(encode(&Image::default()).is_none());
        assert!(encode(&Image::from_rgb8(slint::SharedPixelBuffer::new(161, 160))).is_none());
    }
}
