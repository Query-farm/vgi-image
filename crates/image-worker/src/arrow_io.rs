//! Small Arrow helpers shared across the scalar functions: reading BLOB/VARCHAR
//! input cells, and constructing the MAP(VARCHAR, VARCHAR) output type/array in a
//! way that bind and process agree on exactly.

use std::sync::Arc;

use arrow_array::builder::{MapBuilder, MapFieldNames, StringBuilder};
use arrow_array::cast::AsArray;
use arrow_array::{Array, ArrayRef, MapArray};
use arrow_schema::DataType;
use vgi_rpc::{Result, RpcError};

/// DuckDB-style map child field names (`entries` / `key` / `value`).
pub fn map_field_names() -> MapFieldNames {
    MapFieldNames {
        entry: "entries".to_string(),
        key: "key".to_string(),
        value: "value".to_string(),
    }
}

/// A fresh `MapBuilder<StringBuilder, StringBuilder>` with DuckDB field names.
pub fn map_builder() -> MapBuilder<StringBuilder, StringBuilder> {
    MapBuilder::new(
        Some(map_field_names()),
        StringBuilder::new(),
        StringBuilder::new(),
    )
}

/// The exact `DataType::Map` our [`map_builder`] produces — so `on_bind` can
/// publish an output schema that matches the array built in `process`.
pub fn map_varchar_varchar_type() -> DataType {
    // Finishing an empty builder is the cheapest way to obtain the canonical
    // Map DataType (field nullability, struct child naming, sorted=false).
    let mut b = map_builder();
    b.finish().data_type().clone()
}

/// Borrow the raw bytes of a BLOB/VARCHAR cell at `row`, or `None` if the cell is
/// null. Errors if the column isn't a binary/utf8 type (i.e. not a BLOB input).
pub fn blob_bytes(col: &ArrayRef, row: usize) -> Result<Option<&[u8]>> {
    if col.is_null(row) {
        return Ok(None);
    }
    Ok(Some(match col.data_type() {
        DataType::Binary => col.as_binary::<i32>().value(row),
        DataType::LargeBinary => col.as_binary::<i64>().value(row),
        DataType::Utf8 => col.as_string::<i32>().value(row).as_bytes(),
        DataType::LargeUtf8 => col.as_string::<i64>().value(row).as_bytes(),
        other => {
            return Err(RpcError::value_error(format!(
                "expected a BLOB (binary) argument, got {other:?}"
            )))
        }
    }))
}

/// Append one map row of `(key, value)` string pairs to a map builder.
pub fn append_map_row(
    builder: &mut MapBuilder<StringBuilder, StringBuilder>,
    pairs: &[(String, String)],
) -> Result<()> {
    for (k, v) in pairs {
        builder.keys().append_value(k);
        builder.values().append_value(v);
    }
    builder
        .append(true)
        .map_err(|e| RpcError::runtime_error(e.to_string()))
}

/// Append a NULL map row.
pub fn append_map_null(builder: &mut MapBuilder<StringBuilder, StringBuilder>) -> Result<()> {
    builder
        .append(false)
        .map_err(|e| RpcError::runtime_error(e.to_string()))
}

/// Finish a map builder into an `ArrayRef`.
pub fn finish_map(mut builder: MapBuilder<StringBuilder, StringBuilder>) -> ArrayRef {
    let arr: MapArray = builder.finish();
    Arc::new(arr)
}
