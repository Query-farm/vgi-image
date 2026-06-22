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
mod scalar;

use vgi::Worker;

/// Worker version string, surfaced by `image_version()`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
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

    let mut worker = Worker::new();
    scalar::register(&mut worker);
    worker.run();
}
