//! Pure image logic — no Arrow, no VGI. Everything here works on `&[u8]` blobs
//! and plain Rust types so it can be unit-tested directly. The Arrow adapters in
//! `scalar/` call into these functions.

use std::io::Cursor;

use image::{ColorType, DynamicImage, ImageFormat, ImageReader};
use image_hasher::{HashAlg, HasherConfig};

/// A processing error, rendered to a string for the worker to surface to DuckDB.
#[derive(Debug)]
pub struct ImgError(pub String);

impl std::fmt::Display for ImgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ImgError {}

type Result<T> = std::result::Result<T, ImgError>;

fn err(msg: impl Into<String>) -> ImgError {
    ImgError(msg.into())
}

/// Decoded structural facts about an image, the payload of `image_info`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageInfo {
    pub format: String,
    pub width: u32,
    pub height: u32,
    /// A human-readable color model, e.g. `rgb8`, `rgba8`, `l8`, `la16`.
    pub color: String,
    pub has_alpha: bool,
}

/// Guess the format and read dimensions/color without fully materializing pixels
/// where possible (`image` still decodes the header + color metadata).
pub fn image_info(blob: &[u8]) -> Result<ImageInfo> {
    let reader = ImageReader::new(Cursor::new(blob))
        .with_guessed_format()
        .map_err(|e| err(format!("could not read image header: {e}")))?;
    let format = reader
        .format()
        .map(format_name)
        .unwrap_or("unknown")
        .to_string();
    let img = reader
        .decode()
        .map_err(|e| err(format!("could not decode image: {e}")))?;
    let color = img.color();
    Ok(ImageInfo {
        format,
        width: img.width(),
        height: img.height(),
        color: color_name(color).to_string(),
        has_alpha: color.has_alpha(),
    })
}

/// Canonical lowercase format token for an `ImageFormat`.
fn format_name(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Gif => "gif",
        ImageFormat::WebP => "webp",
        ImageFormat::Tiff => "tiff",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Ico => "ico",
        ImageFormat::Hdr => "hdr",
        ImageFormat::OpenExr => "exr",
        ImageFormat::Pnm => "pnm",
        ImageFormat::Tga => "tga",
        ImageFormat::Dds => "dds",
        ImageFormat::Farbfeld => "farbfeld",
        ImageFormat::Avif => "avif",
        ImageFormat::Qoi => "qoi",
        _ => "unknown",
    }
}

/// Human-readable color-model name.
fn color_name(c: ColorType) -> &'static str {
    match c {
        ColorType::L8 => "l8",
        ColorType::La8 => "la8",
        ColorType::Rgb8 => "rgb8",
        ColorType::Rgba8 => "rgba8",
        ColorType::L16 => "l16",
        ColorType::La16 => "la16",
        ColorType::Rgb16 => "rgb16",
        ColorType::Rgba16 => "rgba16",
        ColorType::Rgb32F => "rgb32f",
        ColorType::Rgba32F => "rgba32f",
        _ => "other",
    }
}

/// One flattened EXIF tag, ready to be dropped into a MAP(VARCHAR, VARCHAR).
pub type ExifPair = (String, String);

/// Parse all EXIF tags into flat `(name, display-value)` pairs. Returns an empty
/// vector when the blob carries no EXIF (not an error — many PNGs have none).
pub fn exif(blob: &[u8]) -> Result<Vec<ExifPair>> {
    let exifdata = match parse_exif(blob) {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    for field in exifdata.fields() {
        let name = field.tag.to_string();
        let value = field.display_value().with_unit(&exifdata).to_string();
        out.push((name, value));
    }
    Ok(out)
}

/// Decoded GPS coordinate, the payload of `exif_gps`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gps {
    pub lat: f64,
    pub lon: f64,
}

/// Extract decimal lat/lon from EXIF GPS tags. `None` when absent or incomplete.
pub fn exif_gps(blob: &[u8]) -> Result<Option<Gps>> {
    let exifdata = match parse_exif(blob) {
        Some(e) => e,
        None => return Ok(None),
    };
    use exif::Tag;
    let lat = gps_coord(&exifdata, Tag::GPSLatitude, Tag::GPSLatitudeRef, b'S');
    let lon = gps_coord(&exifdata, Tag::GPSLongitude, Tag::GPSLongitudeRef, b'W');
    match (lat, lon) {
        (Some(lat), Some(lon)) => Ok(Some(Gps { lat, lon })),
        _ => Ok(None),
    }
}

/// Read a single GPS coordinate (degrees/minutes/seconds → decimal), applying the
/// hemisphere ref tag (`neg_ref` is the byte that flips the sign: `S` or `W`).
fn gps_coord(
    exifdata: &exif::Exif,
    tag: exif::Tag,
    ref_tag: exif::Tag,
    neg_ref: u8,
) -> Option<f64> {
    use exif::{In, Value};
    let field = exifdata.get_field(tag, In::PRIMARY)?;
    let dms = match &field.value {
        Value::Rational(r) if r.len() >= 3 => {
            let d = r[0].to_f64();
            let m = r[1].to_f64();
            let s = r[2].to_f64();
            d + m / 60.0 + s / 3600.0
        }
        _ => return None,
    };
    let sign = match exifdata.get_field(ref_tag, In::PRIMARY) {
        Some(rf) => match &rf.value {
            Value::Ascii(v) if v.first().and_then(|b| b.first()) == Some(&neg_ref) => -1.0,
            _ => 1.0,
        },
        None => 1.0,
    };
    Some(sign * dms)
}

fn parse_exif(blob: &[u8]) -> Option<exif::Exif> {
    let mut cursor = Cursor::new(blob);
    exif::Reader::new().read_from_container(&mut cursor).ok()
}

/// Which perceptual-hash algorithm to compute.
#[derive(Debug, Clone, Copy)]
pub enum HashKind {
    /// Average hash.
    Average,
    /// Difference (gradient) hash.
    Difference,
    /// DCT-based perceptual hash.
    Perceptual,
}

/// Compute a 64-bit perceptual hash of `blob` using `kind`. The 8×8 hash bytes
/// are packed big-endian into a `u64` so Hamming distance over the integers
/// matches bitwise distance over the hash.
pub fn perceptual_hash(blob: &[u8], kind: HashKind) -> Result<u64> {
    let img = decode(blob)?;
    let alg = match kind {
        HashKind::Average => HashAlg::Mean,
        HashKind::Difference => HashAlg::Gradient,
        HashKind::Perceptual => HashAlg::DoubleGradient,
    };
    // 8x8 = 64 bits. For the DCT (perceptual) hash, enable the DCT preprocessing.
    let mut cfg = HasherConfig::new().hash_size(8, 8).hash_alg(alg);
    if matches!(kind, HashKind::Perceptual) {
        cfg = cfg.preproc_dct();
    }
    let hasher = cfg.to_hasher();
    let hash = hasher.hash_image(&img);
    let bytes = hash.as_bytes();
    let mut v: u64 = 0;
    for (i, b) in bytes.iter().take(8).enumerate() {
        v |= (*b as u64) << (8 * (7 - i));
    }
    Ok(v)
}

/// Hamming distance between two packed 64-bit hashes (number of differing bits).
pub fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// An output image format for `thumbnail` / `convert`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutFormat {
    Jpeg,
    Png,
    WebP,
    Gif,
    Bmp,
    Tiff,
}

impl OutFormat {
    /// Parse a case-insensitive format name (`jpeg`/`jpg`, `png`, …).
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "jpeg" | "jpg" => OutFormat::Jpeg,
            "png" => OutFormat::Png,
            "webp" => OutFormat::WebP,
            "gif" => OutFormat::Gif,
            "bmp" => OutFormat::Bmp,
            "tiff" | "tif" => OutFormat::Tiff,
            other => return Err(err(format!("unsupported output format '{other}'"))),
        })
    }

    fn image_format(self) -> ImageFormat {
        match self {
            OutFormat::Jpeg => ImageFormat::Jpeg,
            OutFormat::Png => ImageFormat::Png,
            OutFormat::WebP => ImageFormat::WebP,
            OutFormat::Gif => ImageFormat::Gif,
            OutFormat::Bmp => ImageFormat::Bmp,
            OutFormat::Tiff => ImageFormat::Tiff,
        }
    }
}

/// Resize `blob` to fit within `max_w` × `max_h` preserving aspect ratio, then
/// re-encode to `format`. Never upscales beyond the source resolution.
pub fn thumbnail(blob: &[u8], max_w: u32, max_h: u32, format: OutFormat) -> Result<Vec<u8>> {
    if max_w == 0 || max_h == 0 {
        return Err(err("thumbnail width and height must be positive"));
    }
    let img = decode(blob)?;
    // `thumbnail` preserves aspect ratio and only ever shrinks.
    let thumb = img.thumbnail(max_w, max_h);
    encode(&thumb, format)
}

/// Decode `blob` and re-encode it to `format` (full resolution, no resize).
pub fn convert(blob: &[u8], format: OutFormat) -> Result<Vec<u8>> {
    let img = decode(blob)?;
    encode(&img, format)
}

fn decode(blob: &[u8]) -> Result<DynamicImage> {
    ImageReader::new(Cursor::new(blob))
        .with_guessed_format()
        .map_err(|e| err(format!("could not read image header: {e}")))?
        .decode()
        .map_err(|e| err(format!("could not decode image: {e}")))
}

fn encode(img: &DynamicImage, format: OutFormat) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    // JPEG has no alpha; drop it to RGB8 first so encoding can't fail on RGBA.
    let encoded = if matches!(format, OutFormat::Jpeg) {
        DynamicImage::ImageRgb8(img.to_rgb8())
    } else {
        img.clone()
    };
    encoded
        .write_to(&mut out, format.image_format())
        .map_err(|e| err(format!("could not encode image: {e}")))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    /// Build a tiny PNG in memory: a `w`×`h` image with a diagonal gradient so
    /// perceptual hashes are non-degenerate.
    fn make_png(w: u32, h: u32) -> Vec<u8> {
        let mut img = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = ((x + y) * 255 / (w + h)) as u8;
                img.put_pixel(x, y, Rgb([v, 255 - v, (x * 13 % 256) as u8]));
            }
        }
        let mut buf = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn info_reports_format_and_dimensions() {
        let png = make_png(32, 24);
        let info = image_info(&png).unwrap();
        assert_eq!(info.format, "png");
        assert_eq!(info.width, 32);
        assert_eq!(info.height, 24);
        assert_eq!(info.color, "rgb8");
        assert!(!info.has_alpha);
    }

    #[test]
    fn info_rejects_garbage() {
        assert!(image_info(b"not an image at all").is_err());
    }

    #[test]
    fn exif_absent_is_empty_not_error() {
        let png = make_png(16, 16);
        assert!(exif(&png).unwrap().is_empty());
        assert!(exif_gps(&png).unwrap().is_none());
    }

    #[test]
    fn hashes_roundtrip_and_are_stable() {
        let png = make_png(64, 64);
        // Re-encoding to JPEG should produce a perceptually-close image: small
        // Hamming distance, not identical bits necessarily.
        let jpeg = convert(&png, OutFormat::Jpeg).unwrap();
        for kind in [
            HashKind::Average,
            HashKind::Difference,
            HashKind::Perceptual,
        ] {
            let h1 = perceptual_hash(&png, kind).unwrap();
            let h1_again = perceptual_hash(&png, kind).unwrap();
            assert_eq!(h1, h1_again, "hash must be deterministic");
            let h2 = perceptual_hash(&jpeg, kind).unwrap();
            // JPEG re-encoding of a synthetic gradient perturbs a handful of
            // bits; well under half the 64 bits is still "perceptually close".
            assert!(
                hamming_distance(h1, h2) <= 20,
                "png vs jpeg hash distance too large: {}",
                hamming_distance(h1, h2)
            );
        }
    }

    #[test]
    fn hamming_distance_basics() {
        assert_eq!(hamming_distance(0, 0), 0);
        assert_eq!(hamming_distance(0b1011, 0b0001), 2);
        assert_eq!(hamming_distance(u64::MAX, 0), 64);
    }

    #[test]
    fn thumbnail_preserves_aspect_and_reencodes() {
        let png = make_png(100, 50);
        let thumb = thumbnail(&png, 20, 20, OutFormat::Png).unwrap();
        let info = image_info(&thumb).unwrap();
        assert_eq!(info.format, "png");
        // 100x50 fit into 20x20 → 20x10 (aspect preserved).
        assert_eq!(info.width, 20);
        assert_eq!(info.height, 10);
    }

    #[test]
    fn thumbnail_to_jpeg_works() {
        let png = make_png(80, 80);
        let thumb = thumbnail(&png, 32, 32, OutFormat::Jpeg).unwrap();
        let info = image_info(&thumb).unwrap();
        assert_eq!(info.format, "jpeg");
        assert!(info.width <= 32 && info.height <= 32);
    }

    #[test]
    fn convert_changes_format_keeps_size() {
        let png = make_png(40, 30);
        let bmp = convert(&png, OutFormat::Bmp).unwrap();
        let info = image_info(&bmp).unwrap();
        assert_eq!(info.format, "bmp");
        assert_eq!((info.width, info.height), (40, 30));
    }

    #[test]
    fn out_format_parse() {
        assert_eq!(OutFormat::parse("JPG").unwrap(), OutFormat::Jpeg);
        assert_eq!(OutFormat::parse("png").unwrap(), OutFormat::Png);
        assert!(OutFormat::parse("xyz").is_err());
    }
}
