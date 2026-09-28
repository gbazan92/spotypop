use cosmic::widget::image::Handle;
use image::imageops::FilterType;

/// Pixel sizes of the decoded covers, doubled so they stay sharp on `HiDPI` panels.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
const HERO_PX: u32 = 2 * crate::ui::HERO_SIZE as u32;
const THUMB_PX: u32 = 64;

/// iced is built without image codecs, so covers are decoded and scaled here once.
#[derive(Clone, Debug)]
pub struct Artwork {
    pub url: String,
    pub hero: Handle,
    pub thumb: Handle,
}

pub fn decode(url: String, bytes: &[u8]) -> Result<Artwork, String> {
    let image = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    Ok(Artwork {
        url,
        hero: scaled(&image, HERO_PX),
        thumb: scaled(&image, THUMB_PX),
    })
}

/// A list thumbnail; Spotify's smallest images are already this size.
pub fn decode_thumb(bytes: &[u8]) -> Result<Handle, String> {
    let image = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    Ok(scaled(&image, THUMB_PX))
}

fn scaled(image: &image::DynamicImage, size: u32) -> Handle {
    let rgba = image
        .resize_to_fill(size, size, FilterType::Triangle)
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    Handle::from_rgba(width, height, rgba.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, RgbImage};
    use std::io::Cursor;

    #[test]
    fn decodes_and_squares_covers() {
        let mut png = Vec::new();
        RgbImage::from_pixel(640, 480, image::Rgb([30, 215, 96]))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let art = decode("https://i.scdn.co/image/x".into(), &png).unwrap();
        assert_eq!(art.url, "https://i.scdn.co/image/x");
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(String::new(), b"not an image").is_err());
    }
}
