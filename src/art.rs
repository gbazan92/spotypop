use std::fs;
use std::path::PathBuf;

use cosmic::widget::image::Handle;
use image::imageops::FilterType;
use sha2::{Digest, Sha256};

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
        hero: scaled(&image, HERO_PX, false),
        thumb: scaled(&image, THUMB_PX, true),
    })
}

/// A list thumbnail. Images Spotify already sized at 64 px are not resampled.
pub fn decode_thumb(bytes: &[u8]) -> Result<Handle, String> {
    let image = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    Ok(scaled(&image, THUMB_PX, true))
}

/// Bytes saved from an earlier download. Spotify image URLs do not change.
pub fn cached(url: &str) -> Option<Vec<u8>> {
    let bytes = fs::read(cache_path(url)).ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

/// Disk first, then Spotify. The URL is the cache key.
pub async fn fetch(
    url: String,
    spotify: crate::spotify::Spotify,
) -> Result<Vec<u8>, crate::spotify::Error> {
    if let Some(bytes) = cached(&url) {
        return Ok(bytes);
    }
    let bytes = spotify.artwork(&url).await?;
    store(&url, &bytes);
    Ok(bytes)
}

pub fn store(url: &str, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let path = cache_path(url);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, bytes);
}

fn cache_path(url: &str) -> PathBuf {
    let hex = Sha256::digest(url.as_bytes());
    let name = hex
        .iter()
        .take(16)
        .fold(String::with_capacity(32), |mut out, byte| {
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{byte:02x}"));
            out
        });
    cache_dir().join(name)
}

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("spotypop").join("covers")
}

fn scaled(image: &image::DynamicImage, size: u32, fast: bool) -> Handle {
    let rgba = if image.width() == size && image.height() == size {
        image.to_rgba8()
    } else {
        let filter = if fast {
            FilterType::Nearest
        } else {
            FilterType::Triangle
        };
        image.resize_to_fill(size, size, filter).into_rgba8()
    };
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
