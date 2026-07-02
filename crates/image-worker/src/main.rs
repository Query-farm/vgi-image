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
                crate::meta::keywords_json(
                    "image, images, photo, picture, decode, EXIF, metadata, GPS, geotag, \
                     perceptual hash, phash, dhash, ahash, near-duplicate, deduplication, \
                     thumbnail, resize, convert, png, jpeg, gif, bmp, tiff, webp, BLOB",
                ),
            ),
            (
                "vgi.doc_llm".to_string(),
                "Inspect and transform image BLOBs in SQL: decode a header into \
                 format/width/height/color/alpha, extract EXIF metadata and decimal GPS \
                 coordinates, compute 64-bit perceptual fingerprints and their Hamming distance \
                 for near-duplicate detection, generate aspect-preserving previews, and \
                 re-encode between raster formats (png, jpeg, gif, bmp, tiff, webp). Use for \
                 image cataloguing, deduplication, previewing and EXIF/geotag analysis. List the \
                 schema to discover the exact functions."
                    .to_string(),
            ),
            (
                "vgi.doc_md".to_string(),
                "# image — Image Inspection, EXIF/GPS, Perceptual Hashing & Thumbnails in SQL\n\n\
                 ![image logo](https://avatars.githubusercontent.com/u/49300799?s=240&v=4)\n\n\
                 Decode images, read EXIF and GPS metadata, compute perceptual hashes for \
                 near-duplicate detection, and generate thumbnails directly in DuckDB SQL — no \
                 external tooling or file wrangling required. The `image` worker brings a full \
                 image-processing toolbox to the `img` catalog, turning ordinary `BLOB` columns of \
                 PNG, JPEG, GIF, BMP, TIFF and WebP bytes into queryable, structured data.\n\n\
                 This extension is for anyone cataloguing, deduplicating, or auditing image \
                 collections from SQL: data engineers building media pipelines, analysts hunting \
                 near-duplicate or geotagged photos, and teams that want EXIF and \
                 perceptual-hash features right next to their existing DuckDB tables. Everything \
                 runs in-process over Apache Arrow, so image-derived columns join against the rest \
                 of your data without ever leaving the database.\n\n\
                 ## How it works\n\n\
                 The worker is powered by the pure-Rust \
                 [`image`](https://github.com/image-rs/image) crate \
                 ([docs](https://docs.rs/image)) for decoding, resizing and format conversion; \
                 [`kamadak-exif`](https://github.com/kamadak/exif-rs) \
                 ([docs](https://docs.rs/kamadak-exif)) for parsing EXIF tags and GPS \
                 coordinates; and [`image_hasher`](https://github.com/abonander/img_hash) \
                 ([docs](https://docs.rs/image_hasher)) for 64-bit perceptual hashes. Image bytes \
                 are passed as DuckDB `BLOB` values and `NULL` inputs flow through to `NULL` \
                 results, so the capabilities compose cleanly inside larger queries.\n\n\
                 ## When to reach for it\n\n\
                 Reach for this worker to catalogue a folder of images by their format and \
                 dimensions, to read capture metadata and geotags for auditing or mapping, to \
                 find near-duplicate or reposted photos by perceptual similarity, and to \
                 normalize or preview a mixed set of images — all without leaving SQL or \
                 standing up an external image-processing service. Malformed or truncated bytes \
                 yield a graceful `NULL` or a clear error, so it is safe to run across messy data \
                 at scale. List the schema to discover the exact functions and their signatures."
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
            // VGI152: an analyst-task suite so `vgi-lint simulate` can measure how
            // well an agent actually drives this worker. Each task is a natural
            // prompt plus the canonical `reference_sql` that answers it. Every task
            // is deterministic and self-contained — it builds its own image BLOB
            // inline with `from_hex(...)`, so it needs no external fixtures.
            (
                "vgi.agent_test_tasks".to_string(),
                crate::meta::agent_test_tasks_json(),
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
                    crate::meta::keywords_json(
                        "image, image_info, exif, exif_gps, phash, dhash, ahash, phash_distance, \
                         thumbnail, convert, decode, metadata, GPS, perceptual hash, deduplication, \
                         resize, format conversion",
                    ),
                ),
                // VGI123 classifying tags (bare keys: domain/category/topic) for faceting.
                ("domain".to_string(), "media-and-imaging".to_string()),
                ("category".to_string(), "image-processing".to_string()),
                ("topic".to_string(), "image-inspection-and-transformation".to_string()),
                // VGI413: the schema's category registry — an ordered list of the
                // navigation sections its objects are grouped into. Each function
                // carries a `vgi.category` naming one of these.
                (
                    "vgi.categories".to_string(),
                    r#"[
  {"name": "inspection", "description": "Decode an image header to read its format, pixel dimensions, color model, and alpha channel."},
  {"name": "metadata", "description": "Extract embedded EXIF tags and decimal GPS coordinates from image bytes."},
  {"name": "hashing", "description": "Compute compact perceptual fingerprints and compare them to find visually similar or near-duplicate images."},
  {"name": "transformation", "description": "Resize, thumbnail, and re-encode images between raster formats."},
  {"name": "diagnostics", "description": "Inspect the running worker, such as its build version."}
]"#
                    .to_string(),
                ),
                (
                    "vgi.doc_llm".to_string(),
                    "Image inspection and transformation functions: decode an image header, \
                     extract EXIF metadata and GPS, compute perceptual fingerprints and their \
                     Hamming distance, generate previews, and re-encode between raster formats. \
                     Each function takes an image BLOB; NULL inputs yield NULL results."
                        .to_string(),
                ),
                (
                    "vgi.doc_md".to_string(),
                    "# img.main\n\nThe primary schema of the image worker. It groups its \
                     capabilities into a few areas: **inspection** of an image's header \
                     (format, dimensions, color model), reading embedded **metadata** (EXIF \
                     tags and decimal GPS), perceptual **hashing** for near-duplicate detection, \
                     and image **transformation** (thumbnailing and format conversion).\n\n\
                     Every function takes a single image BLOB (PNG, JPEG, GIF, BMP, TIFF, or \
                     WebP); a NULL input flows through to a NULL result, and undecodable bytes \
                     surface a clear error. Everything runs in-process over Apache Arrow, so \
                     image-derived columns join cleanly against the rest of your data. List the \
                     schema to discover the exact functions and their signatures."
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
