# Image similarity is not file identity

[Open the executed notebook](01_similarity_is_not_file_identity.ipynb). It reads
three related synthetic image files, compares byte identity, image metadata and
three perceptual hashes, then applies an inclusive pHash candidate-review threshold.
The experiment raises that threshold from 8 to 16 differing bits.

## Run from this directory

Install [uv](https://docs.astral.sh/uv/), then:

```bash
uv sync --frozen
uv run python execute.py
```

The locked notebook environment uses Python 3.13–3.14 and published Haybarn
2.0.0rc11. The visible connection uses the pinned native release:

```sql
ATTACH 'img' (TYPE vgi, LOCATION 'github-auto://Query-farm/vgi-image@v0.4.0');
```

VGI downloads the platform asset, verifies its checksum and caches it. No worker
checkout, local build, API key or registered kernel is required. First use needs
network access to package indexes, GitHub release assets and community extensions.
Published release assets cover Apple-silicon macOS and Linux x86-64/arm64. This
whole folder can run independently of the repository.

For an editor, register and select the notebook environment:

```bash
uv run python -m ipykernel install --user --name image-examples \
  --display-name "Python (vgi-image notebooks)"
```

Restart and run all after changing parameters. Connection and analytical SQL are
visible; setup, plotting and generated-prose source are folded. The runner uses
fresh temporary kernels with this folder as cwd, and saves only after success.
Overrides require a separate output destination; it receives fixtures, helpers,
assets and environment/documentation companions. Generated artifacts are ignored.

## Experiment

```bash
uv run python execute.py --parameters '{"MAX_PHASH_DISTANCE":16}' \
  --output-dir validation/distance-16
```

The parameter accepts integer distances 0–64, default 8. A pair is selected when
pHash distance is less than or equal to the threshold. The other hash methods are
comparison measurements, not additional filters. Increasing the threshold can
admit more pairs without changing the original files, metadata or hash values.
Restore 8 for the saved default.

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

## Interpretation

pHash, dHash and aHash produce same-width 64-bit fingerprints with different
algorithms. Hamming distance is the number of differing bits, from 0 to 64.
Equal numeric thresholds are not equally calibrated across algorithms. A zero
perceptual distance can occur for files with different bytes and metadata.
Missing GPS remains distinct from a numeric zero coordinate.

This deliberately related three-file fixture cannot estimate false-positive
rates, recall or a generally useful photo-library threshold. Selected pairs are
candidates for review; nothing is deleted or designated the authoritative copy.
The zero-distance JPEG pair illustrates why metadata needs its own review.

To upgrade, change the literal release tag and repeat the default and experiment
with repository checks. See the pinned [worker README](https://github.com/Query-farm/vgi-image/blob/35a4f5c0d382df0030e5876eaddfe79d3caad265/README.md)
and [STANDARD.md](STANDARD.md).
