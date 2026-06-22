//! Scalar functions exposed by the image worker, registered under `img.main`.

mod exif;
mod hash;
mod info;
mod transform;
mod version;

use vgi::Worker;

/// Register every scalar function on the worker.
pub fn register(worker: &mut Worker) {
    worker.register_scalar(version::ImageVersion);
    worker.register_scalar(info::ImageInfo);
    worker.register_scalar(exif::Exif);
    worker.register_scalar(exif::ExifGps);
    worker.register_scalar(hash::PerceptualHash::phash());
    worker.register_scalar(hash::PerceptualHash::dhash());
    worker.register_scalar(hash::PerceptualHash::ahash());
    worker.register_scalar(hash::PhashDistance);
    worker.register_scalar(transform::Thumbnail);
    worker.register_scalar(transform::Convert);
}
