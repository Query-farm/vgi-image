//! The `image` VGI worker.
//!
//! A standalone binary that DuckDB launches and talks to over Apache Arrow IPC
//! (`ATTACH 'img' (TYPE vgi, LOCATION '…')`). It brings image decoding, EXIF
//! metadata, perceptual hashing, thumbnailing and format conversion to SQL under
//! the catalog `img`, schema `main`:
//!
//! ```sql
//! ATTACH 'img' (TYPE vgi, LOCATION './target/release/image-worker');
//! SET search_path = 'img.main';
//!
//! SELECT image_info(blob).*  FROM photos;   -- format/width/height/color/alpha
//! SELECT phash(blob)         FROM photos;    -- 64-bit DCT perceptual hash
//! SELECT thumbnail(blob, width := 64)        FROM photos;  -- re-encoded BLOB
//! ```
//!
//! Each function group lives in its own module under `scalar/`; the pure image
//! logic lives in `imaging.rs`.

mod arrow_io;
mod imaging;
mod meta;
mod scalar;

use vgi::catalog::{CatSchema, CatalogModel};
use vgi::Worker;

/// Worker version string, surfaced by `image_version()`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Catalog + schema metadata (description, provenance, support) surfaced to
/// DuckDB and the `vgi-lint` metadata-quality linter. The function objects
/// themselves are served from the registered scalars; this only adds
/// catalog/schema-level comments and tags.
fn catalog_metadata(name: &str) -> CatalogModel {
    CatalogModel {
        name: name.to_string(),
        comment: Some(
            "Image decoding, EXIF metadata, perceptual hashing, thumbnailing and \
             format conversion over Apache Arrow."
                .to_string(),
        ),
        tags: vec![
            (
                "vgi.title".to_string(),
                "Image Inspection & Transformation".to_string(),
            ),
            (
                "vgi.keywords".to_string(),
                "image, images, photo, picture, decode, EXIF, metadata, GPS, geotag, \
                 perceptual hash, phash, dhash, ahash, near-duplicate, deduplication, \
                 thumbnail, resize, convert, png, jpeg, gif, bmp, tiff, webp, BLOB"
                    .to_string(),
            ),
            (
                "vgi.doc_llm".to_string(),
                "Inspect and transform image BLOBs in SQL: decode a header into \
                 format/width/height/color/alpha, extract EXIF metadata and decimal GPS \
                 coordinates, compute 64-bit perceptual hashes (phash/dhash/ahash) and their \
                 Hamming distance for near-duplicate detection, generate aspect-preserving \
                 thumbnails, and convert between image formats (png, jpeg, gif, bmp, tiff, webp). \
                 Use for image cataloguing, deduplication, thumbnailing and EXIF/geotag analysis."
                    .to_string(),
            ),
            (
                "vgi.doc_md".to_string(),
                "# image\n\nImage decode, EXIF, perceptual hashing, thumbnailing and format \
                 conversion over Apache Arrow, served to DuckDB/SQL as the `img` catalog.\n\n\
                 ## Scalars\n\n`image_info`, `exif`, `exif_gps`, `phash`, `dhash`, `ahash`, \
                 `phash_distance`, `thumbnail`, `convert`, `image_version`.\n\n## Notes\n\n\
                 Supported formats: png, jpeg, gif, bmp, tiff, webp. Image bytes are passed as \
                 DuckDB `BLOB` values; NULL inputs flow through to NULL results."
                    .to_string(),
            ),
            ("vgi.author".to_string(), "Query.Farm".to_string()),
            (
                "vgi.copyright".to_string(),
                "Copyright 2026 Query Farm LLC - https://query.farm".to_string(),
            ),
            ("vgi.license".to_string(), "MIT".to_string()),
            (
                "vgi.support_contact".to_string(),
                "https://github.com/Query-farm/vgi-image/issues".to_string(),
            ),
            (
                "vgi.support_policy_url".to_string(),
                "https://github.com/Query-farm/vgi-image/blob/main/README.md".to_string(),
            ),
        ],
        source_url: Some("https://github.com/Query-farm/vgi-image".to_string()),
        schemas: vec![CatSchema {
            name: "main".to_string(),
            comment: Some(
                "Image inspection and transformation functions (decode, EXIF, hashing, \
                 thumbnail, convert)."
                    .to_string(),
            ),
            tags: vec![
                ("vgi.title".to_string(), "Image — main".to_string()),
                (
                    "vgi.keywords".to_string(),
                    "image, image_info, exif, exif_gps, phash, dhash, ahash, phash_distance, \
                     thumbnail, convert, decode, metadata, GPS, perceptual hash, deduplication, \
                     resize, format conversion"
                        .to_string(),
                ),
                // VGI123 classifying tags (bare keys: domain/category/topic) for faceting.
                ("domain".to_string(), "media-and-imaging".to_string()),
                ("category".to_string(), "image-processing".to_string()),
                ("topic".to_string(), "image-inspection-and-transformation".to_string()),
                (
                    "vgi.source_url".to_string(),
                    "https://github.com/Query-farm/vgi-image/blob/main/crates/image-worker/src/main.rs"
                        .to_string(),
                ),
                (
                    "vgi.doc_llm".to_string(),
                    "Image inspection and transformation functions: decode an image header, \
                     extract EXIF metadata and GPS, compute perceptual hashes and their Hamming \
                     distance, generate thumbnails, and convert between image formats. Each \
                     function takes an image BLOB; NULL inputs yield NULL results."
                        .to_string(),
                ),
                (
                    "vgi.doc_md".to_string(),
                    "## img.main\n\nImage inspection and transformation functions over Apache \
                     Arrow.\n\nUse these to catalogue images (`image_info`), read capture \
                     metadata (`exif`, `exif_gps`), find near-duplicates \
                     (`phash`/`dhash`/`ahash` + `phash_distance`), and produce derived images \
                     (`thumbnail`, `convert`)."
                        .to_string(),
                ),
                // VGI506 representative example queries for the schema. Image BLOBs are
                // built inline with `from_hex(...)` so each query is self-contained.
                (
                    "vgi.example_queries".to_string(),
                    "SELECT (img.main.image_info(from_hex('89504e470d0a1a0a0000000d4948445200000002000000020802000000fdd49a73000000164944415478da6360608812608862601088121088020009be01a9f633974e0000000049454e44ae426082'))).format;\n\
                     SELECT img.main.phash(from_hex('89504e470d0a1a0a0000000d4948445200000002000000020802000000fdd49a73000000164944415478da6360608812608862601088121088020009be01a9f633974e0000000049454e44ae426082'));\n\
                     SELECT img.main.image_version();"
                        .to_string(),
                ),
            ],
            views: Vec::new(),
            macros: Vec::new(),
            tables: Vec::new(),
        }],
        ..Default::default()
    }
}

fn main() {
    // Logs MUST go to stderr — stdout is the Arrow-IPC channel.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().filter_or("VGI_LOG", "info"))
        .format_timestamp_millis()
        .try_init();

    // The catalog name DuckDB sees in `ATTACH 'img' (TYPE vgi, …)`. Default to
    // `img`, but honor an explicit override so a test harness can rename it.
    if std::env::var_os("VGI_WORKER_CATALOG_NAME").is_none() {
        std::env::set_var("VGI_WORKER_CATALOG_NAME", "img");
    }
    let catalog_name =
        std::env::var("VGI_WORKER_CATALOG_NAME").unwrap_or_else(|_| "img".to_string());

    let mut worker = Worker::new();
    scalar::register(&mut worker);
    worker.set_catalog(catalog_metadata(&catalog_name));
    worker.run();
}
