//! Shared helpers for the per-object discovery/description metadata that the
//! `vgi-lint` strict profile expects on **every** function (and table).
//!
//! Each function surfaces these in its `FunctionMetadata.tags`:
//! - `vgi.title` (VGI124)      — human-friendly display name
//! - `vgi.doc_llm` (VGI112)    — Markdown narrative aimed at LLMs/agents
//! - `vgi.doc_md` (VGI113)     — Markdown narrative for human docs
//! - `vgi.keywords` (VGI126)   — JSON array of search terms/synonyms (VGI138)
//!
//! Per-object `vgi.source_url` is intentionally omitted: VGI139 wants the source
//! link to live only on the catalog object, not repeated on every function.

/// A tiny, self-contained, decodable 2×2 RGB PNG as a hex string. Examples build
/// an image BLOB inline with `from_hex(SAMPLE_PNG_HEX)` so every example query is
/// re-runnable as written — no external files or `read_blob` table function.
pub const SAMPLE_PNG_HEX: &str = "89504e470d0a1a0a0000000d4948445200000002000000020802000000fdd49a73000000164944415478da6360608812608862601088121088020009be01a9f633974e0000000049454e44ae426082";

/// A second, visually distinct tiny 4×4 RGB PNG (hex). Used by the `agent_test`
/// similarity task so the perceptual-hash distance is a genuine, worker-computed
/// non-zero value (not a trivial 0 the analyst could shortcut without querying).
pub const SAMPLE_PNG_HEX_B: &str = "89504e470d0a1a0a0000000d4948445200000004000000040802000000269309290000002e4944415478da1d864901003008c3f246045ad0819acac355c778e4c0c0e1efaba4aa22a627ba6332130bb4f2de03fd2e1413e2c4ad370000000049454e44ae426082";

/// Build `from_hex('<png>')` — an inline DuckDB BLOB expression that decodes to a
/// tiny valid PNG. Use inside example SQL where an image BLOB is required.
pub fn sample_png_expr() -> String {
    format!("from_hex('{SAMPLE_PNG_HEX}')")
}

/// Build the `vgi.agent_test_tasks` suite (VGI152/VGI920) as a JSON string.
///
/// Each task is a natural-language prompt; its canonical `reference_sql` (and
/// grading options) live in the private `vgi-agent-tests.yaml` sidecar at the
/// repo root, keyed by task name, so the graded agent never sees them (VGI416).
/// The `vgi-lint simulate` grader compares an LLM analyst's answer against the
/// reference *result*, so every task is designed to have a single, unambiguous
/// scalar answer that any correct query reproduces (a format string, a pixel
/// count, a boolean, a hash, a distance). Each task carries its own image inline
/// as `from_hex('<png>')`, so the suite needs no external fixtures, and each one
/// forces use of a distinct worker capability.
pub fn agent_test_tasks_json() -> String {
    // A single placeholder keeps the (long) sample-image hex correct in every
    // task; `SAMPLE_PNG_HEX` is the tiny valid 2×2 PNG used across examples.
    // Every task grades on ONE unambiguous value. Scalar-answer tasks set
    // `ignore_column_names` so a correct value under a differently-named column
    // still grades as a pass. Tasks whose natural output is a map, a heuristic
    // hash distance, or a format string that has synonyms ('jpg' vs 'jpeg') are
    // framed as yes/no BOOLEAN predicates — the most robust shape for the LLM
    // agent-simulation grader (an exact compare on those raw outputs is a
    // coin-flip because the analyst rounds, renames, or picks a synonym).
    const TEMPLATE: &str = r#"[
  {"name": "image_format", "prompt": "Report the image format (for example 'png' or 'jpeg') of the image whose raw bytes are the hex string __HEX__."},
  {"name": "image_width", "prompt": "What is the pixel width of the image whose raw bytes are the hex string __HEX__?"},
  {"name": "has_exif", "prompt": "Does the image whose raw bytes are the hex string __HEX__ carry any embedded EXIF metadata tags? Return a single boolean."},
  {"name": "has_gps", "prompt": "Does the image whose raw bytes are the hex string __HEX__ contain GPS location metadata? Return a single boolean."},
  {"name": "perceptual_hash", "prompt": "Compute the 64-bit DCT perceptual hash (the phash function) of the image whose raw bytes are the hex string __HEX__."},
  {"name": "difference_hash", "prompt": "Compute the 64-bit difference hash (the dhash function) of the image whose raw bytes are the hex string __HEX__."},
  {"name": "average_hash", "prompt": "Compute the 64-bit average hash (the ahash function) of the image whose raw bytes are the hex string __HEX__."},
  {"name": "images_are_different", "prompt": "You are given two images as hex byte strings. First image: __HEX__. Second image: __HEXB__. Compute the phash (perceptual hash) of each image separately, pass those two hash values into the phash_distance function to get the Hamming distance between them, and answer whether the two images are perceptually different (is that distance greater than zero). Return a single boolean."},
  {"name": "thumbnail_is_jpeg", "prompt": "Generate a thumbnail of the image whose raw bytes are the hex string __HEX__ using the function's default settings, then confirm whether the resulting thumbnail is a JPEG image. Return a single boolean."},
  {"name": "sized_thumbnail_is_png", "prompt": "Generate a 32x32 PNG thumbnail of the image whose raw bytes are the hex string __HEX__ (choose the size and format explicitly), then confirm whether the resulting thumbnail is a PNG image. Return a single boolean."},
  {"name": "convert_to_bmp", "prompt": "Convert the image whose raw bytes are the hex string __HEX__ to BMP, then confirm whether the converted image is in BMP format. Return a single boolean."}
]"#;
    TEMPLATE
        .replace("__HEXB__", SAMPLE_PNG_HEX_B)
        .replace("__HEX__", SAMPLE_PNG_HEX)
}

/// Minimal JSON string literal (with surrounding quotes). Our inputs contain no
/// control characters, but guard quotes/backslashes for correctness.
fn json_string(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Serialize a comma-separated keyword list as a JSON array of strings, e.g.
/// `"a, b"` → `["a","b"]`. VGI138 requires `vgi.keywords` to be a JSON array,
/// not a comma-separated string.
pub fn keywords_json(keywords: &str) -> String {
    let items: Vec<String> = keywords
        .split(',')
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .map(json_string)
        .collect();
    format!("[{}]", items.join(","))
}

/// Build the `vgi.example_queries` tag as a JSON array of
/// `{"description": ..., "sql": ...}` objects (VGI515).
///
/// The VGI extension's native `duckdb_functions().examples` column is a plain
/// `VARCHAR[]` of SQL strings that cannot carry a per-example description, so an
/// object's illustrative examples must ALSO ride on this tag for every example to
/// be described. Each `(description, sql)` pair becomes one described entry.
pub fn example_queries_tag(examples: &[(&str, String)]) -> (String, String) {
    let items: Vec<String> = examples
        .iter()
        .map(|(desc, sql)| {
            format!(
                "{{\"description\":{},\"sql\":{}}}",
                json_string(desc),
                json_string(sql)
            )
        })
        .collect();
    (
        "vgi.example_queries".to_string(),
        format!("[{}]", items.join(",")),
    )
}

/// Build the four standard per-object discovery/description tags.
///
/// `relative_path` identifies the implementing file (kept in the signature for
/// call-site documentation; the source link itself lives on the catalog object
/// per VGI139, so it is not emitted here).
///
/// `category` names one of the schema's `vgi.categories` (VGI413) — it groups the
/// object under a navigation section for listing, SEO, and discovery.
pub fn object_tags(
    title: &str,
    description_llm: &str,
    description_md: &str,
    keywords: &str,
    category: &str,
    _relative_path: &str,
) -> Vec<(String, String)> {
    vec![
        ("vgi.title".to_string(), title.to_string()),
        ("vgi.doc_llm".to_string(), description_llm.to_string()),
        ("vgi.doc_md".to_string(), description_md.to_string()),
        ("vgi.keywords".to_string(), keywords_json(keywords)),
        // VGI413: place this object in one of the schema's declared categories.
        ("vgi.category".to_string(), category.to_string()),
    ]
}
