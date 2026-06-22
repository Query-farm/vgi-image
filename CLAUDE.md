# CLAUDE.md — vgi-image

Contributor/agent notes. User-facing docs live in `README.md`; this is the
"how it's built and where the sharp edges are" companion.

## What this is

A [VGI](https://query.farm) worker (Rust, compiled binary) exposing image
decode / EXIF / perceptual-hashing / thumbnailing to DuckDB/SQL over Arrow IPC.
Built on the `vgi` crate (crates.io), modeled on `vgi-fixedformat` /
`vgi-crontimes`. Catalog name `img` (single `main` schema).

## Layout

```
Cargo.toml                         workspace; pins vgi = "0.5.0"
crates/image-worker/
  src/main.rs                      Worker::new(); registers scalars
  src/imaging.rs                   PURE logic (no Arrow): decode/exif/gps/hash/thumbnail/convert + unit tests
  src/arrow_io.rs                  BLOB reading + MAP/STRUCT builders + in-process scalar test harness
  src/scalar/{info,exif,hash,transform,version,mod}.rs   thin Arrow adapters, one group each
  examples/gen_fixtures.rs         deterministically generates the test images (make fixtures)
  tests/exif_gps.rs                integration test (hand-built EXIF/JPEG with GPS)
test/sql/*.test                    haybarn-unittest sqllogictest — authoritative E2E
test/sql/data/                     committed tiny fixture images
Makefile                           test / test-unit / test-sql / lint / fmt / fixtures / build / clean
```

Pattern: keep computation in `imaging.rs` (pure, unit-tested), keep
Arrow marshalling in `arrow_io.rs` + `scalar/*.rs` (thin, harness-tested).

## Scalars & named args — sharp edge (read first)

**DuckDB does not bind named args to scalar functions.** `thumbnail(b, width :=
128)` fails the binder. So `thumbnail(b)` uses defaults (128×128 jpeg) and
`convert(b, 'png')` is positional. There is no named-arg option for any scalar
here. (A `dominant_colors` *table* function could use named args, but it's not
implemented — it was deferred; the named-scalar-arg limitation is why.)

## Sharp edges (learned the hard way)

1. **`haybarn-unittest` skips `require vgi`** — `.test` files use explicit
   `LOAD vgi;`. Functions live under the `img` catalog, so each file does
   `SET search_path = 'img.main'`, then `USE memory` before `DETACH`.
2. **The SQL E2E is what exercises the Arrow boundary.** Pure `imaging.rs` unit
   tests can be green while STRUCT/MAP/UBIGINT/BLOB marshalling is wrong — the
   `arrow_io.rs` in-process harness and `test/sql/*` cover that layer. Run both.
3. **MAP/STRUCT DataType must match between bind and process.** `exif` derives
   its `DataType::Map` from an empty `MapBuilder` (DuckDB field names
   `entries`/`key`/`value`) so the bind-time and process-time schemas are
   identical; mismatches surface as obscure cast errors at query time.
4. **MSRV pins.** `image = "=0.25.9"` and `image_hasher = "=3.0.0"` — newer
   patch releases bump MSRV past the workspace `rust-version = 1.86`. `image`
   default features are disabled (drops the heavy AVIF/rav1e chain); only
   png/jpeg/gif/bmp/tiff/webp are enabled. Don't unpin without checking MSRV.
5. **Fixtures are generated, not hand-authored.** `make fixtures` runs
   `examples/gen_fixtures.rs` to (re)produce `test/sql/data/` deterministically,
   including a decodable JPEG with EXIF GPS injected via an APP1 segment.

## Testing

```sh
cargo test --workspace        # pure unit + arrow-boundary harness + integration
cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --all -- --check
make test-sql                 # builds release, sets VGI_IMAGE_WORKER, haybarn-unittest over test/sql/*
make test                     # cargo test + sql
```

`make test-sql` runs `cargo build --release` then points
`VGI_IMAGE_WORKER="$(pwd)/target/release/image-worker"` and runs
`haybarn-unittest --test-dir . "test/sql/*"` (install once: `uv tool install
haybarn-unittest`). CI runs fmt/clippy/build/test plus an `e2e-sql` job.

## Function surface

`image_info` (STRUCT), `exif` (MAP), `exif_gps` (STRUCT, NULL if absent),
`phash`/`dhash`/`ahash` (UBIGINT, 64-bit), `phash_distance` (INT Hamming, pure
integer), `thumbnail` (BLOB, aspect-preserving), `convert` (BLOB),
`image_version` (VARCHAR). Garbage/empty/truncated bytes → graceful NULL or a
clear error (see the boundary tests for exact behavior).
