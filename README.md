<p align="center">
  <img src="https://raw.githubusercontent.com/Query-farm/vgi/main/docs/vgi-logo.png" alt="Vector Gateway Interface (VGI)" width="320">
</p>

<p align="center"><em>A <a href="https://query.farm">Query.Farm</a> VGI worker for DuckDB.</em></p>

# vgi-image

Decode images, read **EXIF**, compute **perceptual hashes**, make **thumbnails**
and **convert formats** inside DuckDB — with SQL, over plain `BLOB` columns.

It runs as a [VGI worker](https://query.farm): a small standalone binary that
DuckDB launches and talks to over Apache Arrow. You `ATTACH` it and call its
functions like any other.

```sql
ATTACH 'img' (TYPE vgi, LOCATION './target/release/image-worker');
SET search_path = 'img.main';

SELECT image_info(data).* FROM read_blob('photos/*.jpg');
-- {'format': jpeg, 'width': 4032, 'height': 3024, 'color': rgb8, 'has_alpha': false}
```

---

## Quick start

**1. Build the worker** (needs Rust 1.86+):

```sh
cargo build --release          # produces target/release/image-worker
```

**2. Attach it in DuckDB** (any DuckDB with the `vgi` community extension):

```sql
INSTALL vgi FROM community;    -- one time
ATTACH 'img' (TYPE vgi, LOCATION '/absolute/path/to/image-worker');
SET search_path = 'img.main';   -- so you can call functions unqualified
```

Use an **absolute** `LOCATION` (it's resolved relative to DuckDB's working
directory). DuckDB's built-in `read_blob('*.jpg')` is the easy way to get image
bytes into a `BLOB` column to feed these functions.

---

## Function catalog

| Function | Shape | What it does |
|----------|-------|--------------|
| `image_info(blob)` | scalar → STRUCT | `format`, `width`, `height`, `color`, `has_alpha` |
| `exif(blob)` | scalar → MAP(VARCHAR, VARCHAR) | All EXIF tags, flattened to a string→string map |
| `exif_gps(blob)` | scalar → STRUCT(lat, lon) | Decimal GPS coordinate, `NULL` if absent |
| `phash(blob)` | scalar → UBIGINT | 64-bit DCT perceptual hash |
| `dhash(blob)` | scalar → UBIGINT | 64-bit difference (gradient) hash |
| `ahash(blob)` | scalar → UBIGINT | 64-bit average hash |
| `phash_distance(a, b)` | scalar → INT | Hamming distance (0–64) between two hashes |
| `thumbnail(blob, width := 128, height := 128, format := 'jpeg')` | scalar → BLOB | Aspect-preserving resize + re-encode |
| `convert(blob, format)` | scalar → BLOB | Decode and re-encode to another format |
| `image_version()` | scalar → VARCHAR | Worker version string |

### `image_info` — inspect without a full pipeline

```sql
SELECT file, (image_info(content)).*
FROM read_blob('photos/*');
```

`color` is the decoded color model (`rgb8`, `rgba8`, `l8`, `la16`, …);
`has_alpha` reflects whether that model carries an alpha channel.

### `exif` / `exif_gps` — metadata

```sql
-- Pull a couple of well-known tags out of the EXIF map:
SELECT
  exif(content)['Model']       AS camera,
  exif(content)['DateTime']    AS shot_at,
  exif_gps(content).lat        AS lat,
  exif_gps(content).lon        AS lon
FROM read_blob('photos/*.jpg');
```

A blob with no EXIF yields an **empty map** (not an error); `exif_gps` returns
`NULL` when GPS tags are missing or incomplete. Longitudes west and latitudes
south come back negative.

### `phash` / `dhash` / `ahash` + `phash_distance` — near-duplicate detection

The three hashes pack an 8×8 (64-bit) perceptual fingerprint into a `UBIGINT`, so
Hamming distance over the integers equals bitwise distance over the hash. Small
distances mean visually similar images.

```sql
-- Find near-duplicates of a reference image (distance ≤ 8 of 64 bits):
WITH ref AS (SELECT phash(content) AS h FROM read_blob('needle.jpg'))
SELECT b.file, phash_distance(phash(b.content), ref.h) AS dist
FROM read_blob('haystack/*') b, ref
WHERE phash_distance(phash(b.content), ref.h) <= 8
ORDER BY dist;
```

`phash` (DCT-based) is the most robust to scaling/compression; `dhash` is cheap
and good at catching crops/edits; `ahash` is the simplest/fastest.

### `thumbnail` / `convert` — resize & re-encode

```sql
-- Write 256px JPEG thumbnails out to files:
COPY (
  SELECT file, thumbnail(content, width := 256, height := 256, format := 'jpeg') AS thumb
  FROM read_blob('photos/*')
) TO 'thumbs' (FORMAT parquet);

-- Convert a PNG column to WebP at full resolution:
SELECT convert(content, 'webp') FROM read_blob('icons/*.png');
```

`thumbnail` only ever **shrinks** and always **preserves aspect ratio** (a
100×50 image into a 128×128 box becomes 128×64). Both functions take/return
`BLOB`. Supported output formats: `jpeg`, `png`, `webp`, `gif`, `bmp`, `tiff`
(JPEG output drops any alpha channel).

---

## Type mapping

| Output | DuckDB type |
|--------|-------------|
| `image_info` | `STRUCT(format VARCHAR, width INT, height INT, color VARCHAR, has_alpha BOOLEAN)` |
| `exif` | `MAP(VARCHAR, VARCHAR)` |
| `exif_gps` | `STRUCT(lat DOUBLE, lon DOUBLE)` |
| `phash` / `dhash` / `ahash` | `UBIGINT` (UInt64) |
| `phash_distance` | `INTEGER` |
| `thumbnail` / `convert` | `BLOB` |

Inputs are `BLOB` columns (`VARCHAR` is also accepted and read as raw bytes).
`NULL` inputs produce `NULL` outputs.

---

## Supported image formats

Decode/encode: **PNG, JPEG, GIF, BMP, TIFF, WebP** (decode for all; encode for
all of these). The heavy AVIF codec chain is intentionally left out to keep the
binary small and the MSRV at 1.86 — add the `image` crate's `avif` feature if you
need it.

---

## Dependencies & licensing

This worker (MIT) is built on:

| Crate | License | Role |
|-------|---------|------|
| [`image`](https://crates.io/crates/image) | MIT/Apache-2.0 | decode / encode / resize |
| [`kamadak-exif`](https://crates.io/crates/kamadak-exif) | BSD-2-Clause | EXIF parsing |
| [`image_hasher`](https://crates.io/crates/image_hasher) | MIT/Apache-2.0 | aHash / dHash / pHash (DCT) |
| [`vgi`](https://crates.io/crates/vgi) | — | VGI worker SDK (Arrow IPC) |

`image` is pinned to `0.25.9` and `image_hasher` to `3.0.0` because their next
patch releases raise the MSRV past this crate's `rust-version` (1.86).

---

## Development

A `Makefile` wraps the common workflows:

```sh
make test-unit   # cargo test --workspace (pure logic + Arrow-boundary tests)
make test-sql    # build the release worker, run the DuckDB SQL E2E suite
make test        # both of the above (the full local gate)
make lint        # cargo clippy -D warnings + cargo fmt --check
make fixtures    # regenerate the committed test/sql/data/* images
```

The underlying commands:

```sh
cargo build --release          # build the worker binary
cargo test --workspace         # unit + integration tests
cargo fmt --all -- --check     # formatting
cargo clippy --all-targets --all-features -- -D warnings
```

The code splits into a pure-logic module (`src/imaging.rs` — all decode/EXIF/
hash/resize/convert logic over `&[u8]`, fully unit-tested) and thin Arrow
adapters (`src/scalar/*`, one module per function group, mirroring the
`fixedformat` worker's layout). `src/arrow_io.rs` holds the shared BLOB-reading
and MAP-building helpers, plus `#[cfg(test)] test_support` helpers that drive a
`ScalarFunction` in-process (build an input batch, run `on_bind` + `process`,
inspect the result array).

### Testing layers

* **Unit / Arrow boundary** (`cargo test`): the pure logic is unit-tested in
  `imaging.rs`; each `scalar/*` module additionally drives its dispatch layer
  in-process — NULL/empty/garbage/truncated BLOBs, NULL array elements, and the
  STRUCT/MAP/UBIGINT/Binary builders producing exactly the `DataType` that
  `on_bind` declares.
* **SQL end-to-end** (`make test-sql`): `test/sql/*.test` are DuckDB
  sqllogictest files run via [`haybarn-unittest`][hb] (`uv tool install
  haybarn-unittest`). They `ATTACH` the compiled worker and exercise every
  function over committed fixture images (`test/sql/data/`), which is the only
  layer that crosses the real Arrow IPC boundary. The fixtures are regenerated
  deterministically by `make fixtures` (the `gen_fixtures` example).

[hb]: https://pypi.org/project/haybarn-unittest/

A hand-crafted EXIF/JPEG blob also exercises the GPS-decode path end to end in
`tests/exif_gps.rs`.

After rebuilding the worker, `DETACH img; ATTACH …` in DuckDB to pick up the new
binary.

---

## Authorship & License

Written by [Query.Farm](https://query.farm).

Copyright 2026 Query Farm LLC - https://query.farm

