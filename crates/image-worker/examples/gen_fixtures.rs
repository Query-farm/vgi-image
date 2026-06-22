//! Generate the committed SQL-test fixture images under `test/sql/data/`.
//!
//! Run with: `cargo run -p image-worker --example gen_fixtures`
//!
//! Produces:
//!   * `gradient.png` — a 160×120 RGB gradient PNG (no EXIF).
//!   * `gradient.jpg` — the same gradient re-encoded as JPEG (no EXIF).
//!   * `gps.jpg` — a real, decodable JPEG carrying an EXIF APP1 segment with GPS
//!     lat/lon (37°48'30"N 122°25'9"W).
//!
//! These are checked in so the SQL E2E suite needs no build step to obtain
//! image bytes; the generator exists only to (re)produce them deterministically.

use std::io::Cursor;
use std::path::Path;

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

fn make_gradient(w: u32, h: u32) -> RgbImage {
    let mut img = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = ((x + y) * 255 / (w + h)) as u8;
            img.put_pixel(x, y, Rgb([v, 255 - v, (x * 13 % 256) as u8]));
        }
    }
    img
}

fn encode(img: &DynamicImage, fmt: ImageFormat) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, fmt).unwrap();
    buf.into_inner()
}

/// Build a little-endian TIFF block: IFD0 → GPS-IFD with N/W lat/lon.
fn build_exif_tiff() -> Vec<u8> {
    fn u16le(v: u16) -> [u8; 2] {
        v.to_le_bytes()
    }
    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&u16le(0x002A));
    tiff.extend_from_slice(&u32le(8));

    // IFD0: single GPS-IFD pointer entry.
    let gps_ifd_offset: u32 = 8 + 2 + 12 + 4;
    tiff.extend_from_slice(&u16le(1));
    tiff.extend_from_slice(&u16le(0x8825));
    tiff.extend_from_slice(&u16le(4));
    tiff.extend_from_slice(&u32le(1));
    tiff.extend_from_slice(&u32le(gps_ifd_offset));
    tiff.extend_from_slice(&u32le(0));

    // GPS IFD: 4 entries (lat ref/lat, lon ref/lon).
    let gps_entries = 4u16;
    let gps_ifd_size = 2 + (gps_entries as u32) * 12 + 4;
    let data_start = gps_ifd_offset + gps_ifd_size;
    let lat_off = data_start;
    let lon_off = data_start + 24;

    tiff.extend_from_slice(&u16le(gps_entries));

    tiff.extend_from_slice(&u16le(0x0001)); // GPSLatitudeRef
    tiff.extend_from_slice(&u16le(2));
    tiff.extend_from_slice(&u32le(2));
    tiff.extend_from_slice(b"N\0\0\0");

    tiff.extend_from_slice(&u16le(0x0002)); // GPSLatitude
    tiff.extend_from_slice(&u16le(5));
    tiff.extend_from_slice(&u32le(3));
    tiff.extend_from_slice(&u32le(lat_off));

    tiff.extend_from_slice(&u16le(0x0003)); // GPSLongitudeRef
    tiff.extend_from_slice(&u16le(2));
    tiff.extend_from_slice(&u32le(2));
    tiff.extend_from_slice(b"W\0\0\0");

    tiff.extend_from_slice(&u16le(0x0004)); // GPSLongitude
    tiff.extend_from_slice(&u16le(5));
    tiff.extend_from_slice(&u32le(3));
    tiff.extend_from_slice(&u32le(lon_off));

    tiff.extend_from_slice(&u32le(0));

    let push_rat = |buf: &mut Vec<u8>, n: u32, d: u32| {
        buf.extend_from_slice(&u32le(n));
        buf.extend_from_slice(&u32le(d));
    };
    push_rat(&mut tiff, 37, 1);
    push_rat(&mut tiff, 48, 1);
    push_rat(&mut tiff, 30, 1);
    push_rat(&mut tiff, 122, 1);
    push_rat(&mut tiff, 25, 1);
    push_rat(&mut tiff, 9, 1);

    tiff
}

/// Insert an EXIF APP1 segment (with GPS) just after the JPEG SOI marker of an
/// already-encoded baseline JPEG. Returns a new, still-decodable JPEG.
fn inject_exif(jpeg: &[u8]) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "expected JPEG SOI");
    let tiff = build_exif_tiff();
    let mut payload = Vec::new();
    payload.extend_from_slice(b"Exif\0\0");
    payload.extend_from_slice(&tiff);

    let mut out = Vec::new();
    out.extend_from_slice(&[0xFF, 0xD8]); // SOI
    out.extend_from_slice(&[0xFF, 0xE1]); // APP1
    let len = (payload.len() + 2) as u16;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&jpeg[2..]); // rest of the original JPEG
    out
}

fn write(path: &str, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    println!("wrote {path} ({} bytes)", bytes.len());
}

fn main() {
    let dir = Path::new("test/sql/data");
    std::fs::create_dir_all(dir).unwrap();

    let grad = make_gradient(160, 120);
    let png = encode(&DynamicImage::ImageRgb8(grad.clone()), ImageFormat::Png);
    write("test/sql/data/gradient.png", &png);

    let jpg = encode(&DynamicImage::ImageRgb8(grad.clone()), ImageFormat::Jpeg);
    write("test/sql/data/gradient.jpg", &jpg);

    let gps = inject_exif(&jpg);
    write("test/sql/data/gps.jpg", &gps);
}
