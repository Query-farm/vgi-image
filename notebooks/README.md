# Image Toolkit examples

| Notebook | Question | One-parameter experiment |
| --- | --- | --- |
| [01 · Similarity is not file identity](01_similarity_is_not_file_identity.ipynb) | Which visually related files still have different bytes and metadata? | Inclusive pHash threshold 8 → 16 bits |
| [02 · Dimensions, bytes and metadata](02_dimensions_bytes_and_metadata_are_separate.ipynb) | What survives a thumbnail or full-size format conversion? | Explicit PNG box 80 → 240 pixels |

Read saved outputs directly on GitHub, or use each notebook's **Open in Colab** button and run the install cell, then Run All. A single downloaded notebook works: shared `prepare("image")` retrieves and SHA-256-verifies the public synthetic images and provenance at immutable commit `c6e37849c910c7ccfe773373d201df89c8a062e2`. No source checkout, Rust build, API key or photo library is needed. Actual hosted Colab has not been tested; one fresh preloaded Python 3.12 notebook-only execution checks the shared launch/companion path used by both examples.

## Locked local runner

Install [uv](https://docs.astral.sh/uv/), then run from this directory:

```sh
uv sync --locked --extra runner
uv run --locked --extra runner vgi-notebooks
```

Python 3.11+ is supported; uv can install it. The base [shared tools v0.3.14](https://github.com/Query-farm/vgi-example-notebooks/tree/v0.3.14) supply compact setup, exact branding and SVG presentation; the optional `[runner]` extra provides local atomic execution. Colab installs only the base package and accepts compatible preinstalled plotting/data libraries. Haybarn 2.0.0rc11 runs visible SQL:

```sql
ATTACH 'img' (TYPE vgi, LOCATION 'github-auto://Query-farm/vgi-image@v0.4.0');
```

VGI downloads the platform-native binary, verifies the published checksum and caches it. Initial setup needs network access for packages, extensions, companions and release assets; analytical operations use only the local synthetic files. `connect(globals())` starts an in-memory database with empty temporary SQL-secret storage, persistent secrets disabled and no JupySQL default-connection autoload. Cleanup closes the connection and deletes its secret directory.

The shared runner uses a temporary kernel and preserves successful outputs if a later run fails. Defaults retain native SVG MIME and content-addressed `charts/` assets with ordinary GitHub image links; experiments retain native SVG without unpublished image URLs. Optional SVG/PNG review exports and experiment copies are ignored under `artifacts/` and `validation/`. The contact sheets contain original or worker-returned raster image bytes inside SVG figures; Pillow only renders them, without supplying analytical measurements or transformations.

Optional local kernel:

```sh
uv run --locked --extra runner python -m ipykernel install --user --name image-examples --display-name "Python (vgi-image notebooks)"
```

## Separate experiments

```sh
uv run --locked --extra runner vgi-notebooks 01_similarity_is_not_file_identity.ipynb --parameters '{"MAX_PHASH_DISTANCE":16}' --output-dir validation/distance-16
uv run --locked --extra runner vgi-notebooks 02_dimensions_bytes_and_metadata_are_separate.ipynb --parameters '{"MAX_EDGE":240}' --output-dir validation/edge-240
```

Overrides require a separate destination. Restart and run all after editing manually. The pHash threshold accepts integers 0–64 and is inclusive; the parameterized box accepts 80 or 240 pixels. Restore 8 bits / 80 pixels for the defaults.

The first experiment changes review eligibility only: bytes, metadata, fingerprints and distances stay fixed. It admits three pairs instead of one. The second changes only the explicit PNG rendition; source JPEG, default JPEG thumbnail and full-size BMP remain unchanged. The fitted PNG grows from 80×60 to 240×180 in the pinned release. Pixel dimensions, encoded bytes and EXIF presence are assessed separately; no file is deleted or declared the authoritative source.

## Released behavior and current interface

The v0.4.0 release predates the worker's **no-upscale fix**. Its `thumbnail` and `thumbnail_fit` can enlarge sources that fit inside the requested box. Current public source at `c6e37849c910c7ccfe773373d201df89c8a062e2` adds a guard that re-encodes those images at their original size. The second notebook deliberately measures the historical release behavior; its larger-box experiment would change under the newer implementation. Both default transformations shrink this source and are unaffected by that guard. Upgrade the literal release pin deliberately and rerun the artifact checks.

The repository also documents a VGI constant-argument expression issue: calls to the same scalar on the same BLOB column with different constant options in one SELECT can collapse to one expression. Each transform here uses its own materializing SELECT, and the returned formats/dimensions are verified. No engine or worker code was changed.

[coverage.json](coverage.json) maps all **ten named scalar surfaces** to visible executed SQL and interpreted results across the two stories. Actual v0.4.0 function/table/view exports include the three macro-generated hash names that static scans can miss. Current registration and the full release-to-current source diff are retained under [coverage/](coverage/). Coverage is **10/10 released and 10/10 current interface**, with **ten exact released-runtime examples and zero exact current-runtime examples**. Current coverage is an interface compatibility review, explicitly documenting the thumbnail behavior difference; it is not a claim that current source ran or that all modes/encoders were tested.

Existing green [source CI 43571776028](https://github.com/Query-farm/vgi-image/actions/runs/43571776028) is reused for this notebook-only change. No worker rebuild, unit/transport suite or container workflow is repeated.

## Exact fixture provenance

The three files under [fixtures/](fixtures/) are copied **unchanged** from
`test/sql/data/` at public revision
`35a4f5c0d382df0030e5876eaddfe79d3caad265`. Exact byte lengths, SHA-256 hashes and
the source generator path are in [provenance.json](fixtures/provenance.json).
The notebook verifies those hashes before reading through SQL.

The repository generator creates a 160×120 RGB gradient, encodes a PNG and a JPEG,
and injects an EXIF APP1 GPS segment into the JPEG to produce `gps.jpg`. These are
synthetic test images, not real photographs. The injected GPS coordinates are
invented test data, not a person's location or an observed capture site. The
fixtures have no claimed capture date. The generator establishes this family
relationship; perceptual hashes alone would not prove it.

All image metadata and perceptual fingerprints are returned by the real worker.
SQL reads BLOBs, computes byte SHA-256 values, builds unique pairs and applies the
review rule. Pillow decodes SQL-returned bytes only for the displayed contact
sheet; it does not edit, re-encode or supply analytical image measurements.

## Interpretation limits

pHash, dHash and aHash are different 64-bit fingerprints. Hamming distances span 0–64 bits, but equal numeric thresholds are not equally calibrated across algorithms. The heatmap uses the observed distance range for contrast and states that range explicitly. Zero perceptual distance does not establish identical bytes or metadata. These deliberately related files cannot estimate false positives, recall or a generally useful library threshold.

Re-encoding these fixtures returns empty EXIF maps and absent GPS through this reader. That measured outcome is not a universal metadata-sanitization or privacy guarantee. Compression format/settings, prior JPEG loss and container overhead differ, so this is not a controlled image-quality benchmark. Preserve source files and inspect output dimensions, storage size and metadata separately.

See the [pinned worker README](https://github.com/Query-farm/vgi-image/blob/v0.4.0/README.md) and the [shared notebook standard](https://github.com/Query-farm/vgi-example-notebooks/blob/v0.3.14/docs/standard.md).
