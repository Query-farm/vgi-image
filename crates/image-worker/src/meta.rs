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

/// Build `from_hex('<png>')` — an inline DuckDB BLOB expression that decodes to a
/// tiny valid PNG. Use inside example SQL where an image BLOB is required.
pub fn sample_png_expr() -> String {
    format!("from_hex('{SAMPLE_PNG_HEX}')")
}

/// Serialize a comma-separated keyword list as a JSON array of strings, e.g.
/// `"a, b"` → `["a","b"]`. VGI138 requires `vgi.keywords` to be a JSON array,
/// not a comma-separated string.
pub fn keywords_json(keywords: &str) -> String {
    let items: Vec<String> = keywords
        .split(',')
        .map(|k| k.trim())
        .filter(|k| !k.is_empty())
        .map(|k| {
            // Minimal JSON string escaping (keywords contain no control chars,
            // but guard quotes/backslashes for correctness).
            let escaped = k.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Build the four standard per-object discovery/description tags.
///
/// `relative_path` identifies the implementing file (kept in the signature for
/// call-site documentation; the source link itself lives on the catalog object
/// per VGI139, so it is not emitted here).
pub fn object_tags(
    title: &str,
    description_llm: &str,
    description_md: &str,
    keywords: &str,
    _relative_path: &str,
) -> Vec<(String, String)> {
    vec![
        ("vgi.title".to_string(), title.to_string()),
        ("vgi.doc_llm".to_string(), description_llm.to_string()),
        ("vgi.doc_md".to_string(), description_md.to_string()),
        ("vgi.keywords".to_string(), keywords_json(keywords)),
    ]
}
