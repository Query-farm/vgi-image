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

/// Test-only helpers shared by the scalar Arrow-boundary unit tests. These let a
/// `#[cfg(test)]` block drive a `ScalarFunction` end to end in-process (build a
/// one-column input `RecordBatch`, run `on_bind` + `process`, inspect the result)
/// without the RPC/IPC plumbing.
#[cfg(test)]
pub mod test_support {
    use std::sync::Arc;

    use arrow_array::builder::BinaryBuilder;
    use arrow_array::{ArrayRef, RecordBatch};
    use arrow_schema::{Field, Schema, SchemaRef};
    use vgi::arguments::Arguments;
    use vgi::{BindParams, ProcessParams, ScalarFunction};
    use vgi_rpc::Result;

    /// A tiny in-memory PNG (`w`×`h`, diagonal gradient) for decode-path tests.
    pub fn make_png(w: u32, h: u32) -> Vec<u8> {
        use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
        use std::io::Cursor;
        let mut img = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let v = ((x + y) * 255 / (w + h)) as u8;
                img.put_pixel(x, y, Rgb([v, 255 - v, (x * 13 % 256) as u8]));
            }
        }
        let mut buf = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    /// A single-column `Binary` (BLOB) input batch. `None` entries become NULLs.
    pub fn blob_batch(rows: &[Option<&[u8]>]) -> RecordBatch {
        let mut b = BinaryBuilder::new();
        for r in rows {
            match r {
                Some(bytes) => b.append_value(bytes),
                None => b.append_null(),
            }
        }
        let arr: ArrayRef = Arc::new(b.finish());
        let schema = Arc::new(Schema::new(vec![Field::new(
            "blob",
            arr.data_type().clone(),
            true,
        )]));
        RecordBatch::try_new(schema, vec![arr]).unwrap()
    }

    /// Build a `ProcessParams` carrying the given output schema and arguments.
    pub fn process_params(output_schema: SchemaRef, arguments: Arguments) -> ProcessParams {
        ProcessParams {
            output_schema,
            input_schema: None,
            execution_id: Vec::new(),
            init_opaque_data: Vec::new(),
            arguments,
            settings: Default::default(),
            secrets: Default::default(),
            auth_principal: None,
            projection_ids: None,
            pushdown_filters: None,
            join_keys: Vec::new(),
            storage: None,
            order_by_column: None,
            order_by_direction: None,
            order_by_null_order: None,
            order_by_limit: None,
            tablesample_percentage: None,
            tablesample_seed: None,
            attach_opaque_data: None,
            at_unit: None,
            at_value: None,
            copy_from: None,
        }
    }

    /// Run a scalar function over a `Binary` input batch: call `on_bind` to obtain
    /// the declared output schema, then `process`, returning the single result
    /// column. The `arguments` apply to both bind and process.
    pub fn run_scalar<F: ScalarFunction>(
        f: &F,
        rows: &[Option<&[u8]>],
        arguments: Arguments,
    ) -> Result<ArrayRef> {
        let batch = blob_batch(rows);
        let bind = BindParams {
            input_schema: Some(batch.schema()),
            arguments: arguments.clone(),
            ..Default::default()
        };
        let bound = f.on_bind(&bind)?;
        let params = process_params(bound.output_schema.clone(), arguments);
        let out = f.process(&params, &batch)?;
        Ok(out.column(0).clone())
    }

    /// The declared output `DataType` from `on_bind` for a single-BLOB-arg scalar.
    pub fn bound_type<F: ScalarFunction>(f: &F) -> arrow_schema::DataType {
        let bind = BindParams::default();
        let bound = f.on_bind(&bind).unwrap();
        bound.output_schema.field(0).data_type().clone()
    }
}
