use image::{ImageBuffer, ImageEncoder, Rgb, codecs::png::PngEncoder};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasPreset {
    Square,
    Portrait,
    Landscape,
}

impl CanvasPreset {
    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Square => (1024, 1024),
            Self::Portrait => (1080, 1920),
            Self::Landscape => (1920, 1080),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ColorParseError {
    #[error("background color must use #RRGGBB format")]
    InvalidFormat,
}

pub fn parse_hex_color(hex: &str) -> Result<Rgb<u8>, ColorParseError> {
    let Some(value) = hex.strip_prefix('#') else {
        return Err(ColorParseError::InvalidFormat);
    };

    if value.len() != 6 || !value.chars().all(|char| char.is_ascii_hexdigit()) {
        return Err(ColorParseError::InvalidFormat);
    }

    let red = u8::from_str_radix(&value[0..2], 16).map_err(|_| ColorParseError::InvalidFormat)?;
    let green = u8::from_str_radix(&value[2..4], 16).map_err(|_| ColorParseError::InvalidFormat)?;
    let blue = u8::from_str_radix(&value[4..6], 16).map_err(|_| ColorParseError::InvalidFormat)?;

    Ok(Rgb([red, green, blue]))
}

pub fn generate_canvas_png(
    preset: CanvasPreset,
    background: Rgb<u8>,
) -> image::ImageResult<Vec<u8>> {
    let (width, height) = preset.dimensions();
    let image = ImageBuffer::from_pixel(width, height, background);
    let mut bytes = Vec::new();

    PngEncoder::new(&mut bytes).write_image(
        image.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )?;

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_presets_to_dimensions() {
        assert_eq!(CanvasPreset::Square.dimensions(), (1024, 1024));
        assert_eq!(CanvasPreset::Portrait.dimensions(), (1080, 1920));
        assert_eq!(CanvasPreset::Landscape.dimensions(), (1920, 1080));
    }

    #[test]
    fn parses_hex_color() {
        assert_eq!(parse_hex_color("#00aAFF").unwrap(), Rgb([0, 170, 255]));
        assert_eq!(
            parse_hex_color("00aAFF"),
            Err(ColorParseError::InvalidFormat)
        );
        assert_eq!(
            parse_hex_color("#00aa"),
            Err(ColorParseError::InvalidFormat)
        );
        assert_eq!(
            parse_hex_color("#00xxff"),
            Err(ColorParseError::InvalidFormat)
        );
    }

    #[test]
    fn generates_png_with_expected_dimensions_and_color() {
        let bytes = generate_canvas_png(CanvasPreset::Square, Rgb([12, 34, 56])).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().into_rgb8();

        assert_eq!(decoded.dimensions(), (1024, 1024));
        assert_eq!(*decoded.get_pixel(0, 0), Rgb([12, 34, 56]));
        assert_eq!(*decoded.get_pixel(1023, 1023), Rgb([12, 34, 56]));
    }
}
