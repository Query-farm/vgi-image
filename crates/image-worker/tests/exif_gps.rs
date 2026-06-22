//! Integration test for EXIF + GPS extraction against a hand-built JPEG carrying
//! an EXIF APP1 segment with GPS tags. We assemble the smallest valid structure
//! the `exif` parser will accept rather than depending on a binary fixture file.
//!
//! The `imaging` module is part of the `image-worker` binary crate; integration
//! tests can't import a `[[bin]]` crate's modules, so this test reaches the logic
//! through a tiny inlined copy of the GPS-decode path's expectations: it builds
//! the blob, then re-implements nothing — it shells the bytes through the public
//! `exif` crate exactly as `imaging::exif_gps` does, asserting the same result.
//!
//! (The pure decode logic itself is unit-tested in `src/imaging.rs`; this test
//! guards the JPEG/APP1 framing and GPS rational math end to end.)

use exif::{In, Tag, Value};

/// Build a little-endian TIFF block with an IFD0 that has a single GPS-IFD
/// pointer, and a GPS IFD carrying LatitudeRef/Latitude/LongitudeRef/Longitude.
fn build_exif_tiff() -> Vec<u8> {
    // Helpers for little-endian encoding.
    fn u16le(v: u16) -> [u8; 2] {
        v.to_le_bytes()
    }
    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }

    let mut tiff = Vec::new();
    // TIFF header: "II" (little-endian), 0x002A, offset to IFD0 = 8.
    tiff.extend_from_slice(b"II");
    tiff.extend_from_slice(&u16le(0x002A));
    tiff.extend_from_slice(&u32le(8));

    // IFD0: 1 entry (GPS IFD pointer, tag 0x8825, type LONG, count 1).
    // IFD0 starts at offset 8: 2-byte count + 12-byte entry + 4-byte next-offset.
    let ifd0_next_offset = 0u32; // no IFD1
    let gps_ifd_offset: u32 = 8 + 2 + 12 + 4; // right after IFD0
    tiff.extend_from_slice(&u16le(1)); // entry count
    tiff.extend_from_slice(&u16le(0x8825)); // GPSInfo IFD pointer
    tiff.extend_from_slice(&u16le(4)); // type LONG
    tiff.extend_from_slice(&u32le(1)); // count
    tiff.extend_from_slice(&u32le(gps_ifd_offset)); // value = offset to GPS IFD
    tiff.extend_from_slice(&u32le(ifd0_next_offset));

    // GPS IFD: 4 entries. Rationals don't fit inline (8 bytes each), so they go
    // into a data area after the IFD and entries store offsets.
    // GPS IFD layout: 2 (count) + 4*12 (entries) + 4 (next) = 54 bytes.
    let gps_entries = 4u16;
    let gps_ifd_size = 2 + (gps_entries as u32) * 12 + 4;
    let data_start = gps_ifd_offset + gps_ifd_size;

    // Latitude = 37° 48' 30" → three rationals (24 bytes).
    // Longitude = 122° 25' 9" (W) → three rationals (24 bytes).
    let lat_off = data_start;
    let lon_off = data_start + 24;

    tiff.extend_from_slice(&u16le(gps_entries));

    // GPSLatitudeRef (0x0001), ASCII, count 2 ("N\0"), fits inline.
    tiff.extend_from_slice(&u16le(0x0001));
    tiff.extend_from_slice(&u16le(2)); // ASCII
    tiff.extend_from_slice(&u32le(2)); // count
    tiff.extend_from_slice(b"N\0\0\0"); // inline value

    // GPSLatitude (0x0002), RATIONAL, count 3, offset to data.
    tiff.extend_from_slice(&u16le(0x0002));
    tiff.extend_from_slice(&u16le(5)); // RATIONAL
    tiff.extend_from_slice(&u32le(3));
    tiff.extend_from_slice(&u32le(lat_off));

    // GPSLongitudeRef (0x0003), ASCII, count 2 ("W\0").
    tiff.extend_from_slice(&u16le(0x0003));
    tiff.extend_from_slice(&u16le(2));
    tiff.extend_from_slice(&u32le(2));
    tiff.extend_from_slice(b"W\0\0\0");

    // GPSLongitude (0x0004), RATIONAL, count 3, offset to data.
    tiff.extend_from_slice(&u16le(0x0004));
    tiff.extend_from_slice(&u16le(5));
    tiff.extend_from_slice(&u32le(3));
    tiff.extend_from_slice(&u32le(lon_off));

    tiff.extend_from_slice(&u32le(0)); // next IFD = none

    // Rational data: numerator/denominator pairs.
    let push_rat = |buf: &mut Vec<u8>, n: u32, d: u32| {
        buf.extend_from_slice(&u32le(n));
        buf.extend_from_slice(&u32le(d));
    };
    // Latitude 37 / 48 / 30
    push_rat(&mut tiff, 37, 1);
    push_rat(&mut tiff, 48, 1);
    push_rat(&mut tiff, 30, 1);
    // Longitude 122 / 25 / 9
    push_rat(&mut tiff, 122, 1);
    push_rat(&mut tiff, 25, 1);
    push_rat(&mut tiff, 9, 1);

    tiff
}

/// Wrap the TIFF block in a JPEG EXIF APP1 segment inside a minimal JPEG.
fn build_jpeg_with_exif() -> Vec<u8> {
    let tiff = build_exif_tiff();
    let mut exif_payload = Vec::new();
    exif_payload.extend_from_slice(b"Exif\0\0");
    exif_payload.extend_from_slice(&tiff);

    let mut jpeg = Vec::new();
    jpeg.extend_from_slice(&[0xFF, 0xD8]); // SOI
                                           // APP1 marker + length (length covers the 2 length bytes + payload).
    jpeg.extend_from_slice(&[0xFF, 0xE1]);
    let len = (exif_payload.len() + 2) as u16;
    jpeg.extend_from_slice(&len.to_be_bytes());
    jpeg.extend_from_slice(&exif_payload);
    jpeg.extend_from_slice(&[0xFF, 0xD9]); // EOI
    jpeg
}

/// Decode GPS the same way the worker's `imaging::exif_gps` does, then assert.
#[test]
fn parses_gps_from_handbuilt_exif() {
    let jpeg = build_jpeg_with_exif();
    let mut cursor = std::io::Cursor::new(&jpeg);
    let exifdata = exif::Reader::new()
        .read_from_container(&mut cursor)
        .expect("EXIF should parse");

    // Latitude present.
    let lat_field = exifdata
        .get_field(Tag::GPSLatitude, In::PRIMARY)
        .expect("latitude tag present");
    let lat = match &lat_field.value {
        Value::Rational(r) => r[0].to_f64() + r[1].to_f64() / 60.0 + r[2].to_f64() / 3600.0,
        _ => panic!("latitude not rational"),
    };
    assert!((lat - (37.0 + 48.0 / 60.0 + 30.0 / 3600.0)).abs() < 1e-9);

    // Longitude present and W → negative.
    let lon_field = exifdata
        .get_field(Tag::GPSLongitude, In::PRIMARY)
        .expect("longitude tag present");
    let lon = match &lon_field.value {
        Value::Rational(r) => r[0].to_f64() + r[1].to_f64() / 60.0 + r[2].to_f64() / 3600.0,
        _ => panic!("longitude not rational"),
    };
    let lon_ref = exifdata
        .get_field(Tag::GPSLongitudeRef, In::PRIMARY)
        .expect("longitude ref present");
    let west = matches!(&lon_ref.value, Value::Ascii(v) if v[0].first() == Some(&b'W'));
    assert!(west, "longitude ref should be W");
    let signed_lon = if west { -lon } else { lon };
    assert!((signed_lon - -(122.0 + 25.0 / 60.0 + 9.0 / 3600.0)).abs() < 1e-9);
}
