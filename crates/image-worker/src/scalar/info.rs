//! `image_info(blob)` → `STRUCT(format VARCHAR, width INT, height INT,
//! color VARCHAR, has_alpha BOOLEAN)`.

use std::sync::Arc;

use arrow_array::builder::{BooleanBuilder, Int32Builder, StringBuilder};
use arrow_array::{ArrayRef, RecordBatch, StructArray};
use arrow_buffer::NullBuffer;
use arrow_schema::{DataType, Field, Fields};
use vgi::{ArgSpec, BindParams, BindResponse, FunctionMetadata, ProcessParams, ScalarFunction};
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::blob_bytes;
use crate::imaging;

pub struct ImageInfo;

/// The fixed output STRUCT fields.
fn struct_fields() -> Fields {
    Fields::from(vec![
        Field::new("format", DataType::Utf8, true),
        Field::new("width", DataType::Int32, true),
        Field::new("height", DataType::Int32, true),
        Field::new("color", DataType::Utf8, true),
        Field::new("has_alpha", DataType::Boolean, true),
    ])
}

impl ScalarFunction for ImageInfo {
    fn name(&self) -> &str {
        "image_info"
    }

    fn metadata(&self) -> FunctionMetadata {
        FunctionMetadata {
            description:
                "Decode an image BLOB's header into a STRUCT(format, width, height, color, has_alpha)"
                    .into(),
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![ArgSpec::any_column("blob", 0, "Image bytes (BLOB)")]
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Struct(struct_fields())))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let col = batch.column(0);
        let rows = batch.num_rows();

        let mut format = StringBuilder::new();
        let mut width = Int32Builder::new();
        let mut height = Int32Builder::new();
        let mut color = StringBuilder::new();
        let mut has_alpha = BooleanBuilder::new();
        let mut valid: Vec<bool> = Vec::with_capacity(rows);

        for i in 0..rows {
            match blob_bytes(col, i)? {
                None => {
                    format.append_null();
                    width.append_null();
                    height.append_null();
                    color.append_null();
                    has_alpha.append_null();
                    valid.push(false);
                }
                Some(bytes) => {
                    let info = imaging::image_info(bytes)
                        .map_err(|e| RpcError::value_error(e.to_string()))?;
                    format.append_value(&info.format);
                    width.append_value(info.width as i32);
                    height.append_value(info.height as i32);
                    color.append_value(&info.color);
                    has_alpha.append_value(info.has_alpha);
                    valid.push(true);
                }
            }
        }

        let arrays: Vec<ArrayRef> = vec![
            Arc::new(format.finish()),
            Arc::new(width.finish()),
            Arc::new(height.finish()),
            Arc::new(color.finish()),
            Arc::new(has_alpha.finish()),
        ];
        let out: ArrayRef = Arc::new(StructArray::new(
            struct_fields(),
            arrays,
            Some(NullBuffer::from(valid)),
        ));
        RecordBatch::try_new(params.output_schema.clone(), vec![out])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow_io::test_support::{bound_type, make_png, run_scalar};
    use arrow_array::cast::AsArray;
    use arrow_array::types::Int32Type;
    use arrow_array::Array;
    use vgi::arguments::Arguments;

    #[test]
    fn bind_declares_the_struct_the_process_builds() {
        // The DataType published at bind must equal the array's at process.
        assert_eq!(bound_type(&ImageInfo), DataType::Struct(struct_fields()));
    }

    #[test]
    fn process_decodes_a_png_into_struct_fields() {
        let png = make_png(20, 12);
        let out = run_scalar(&ImageInfo, &[Some(&png)], Arguments::default()).unwrap();
        let s = out.as_struct();
        assert_eq!(out.data_type(), &DataType::Struct(struct_fields()));
        assert_eq!(s.column(0).as_string::<i32>().value(0), "png");
        assert_eq!(s.column(1).as_primitive::<Int32Type>().value(0), 20);
        assert_eq!(s.column(2).as_primitive::<Int32Type>().value(0), 12);
    }

    #[test]
    fn null_element_yields_null_struct_row() {
        let png = make_png(8, 8);
        let out = run_scalar(&ImageInfo, &[Some(&png), None], Arguments::default()).unwrap();
        assert!(!out.is_null(0));
        assert!(out.is_null(1), "NULL input must produce a NULL struct row");
    }

    #[test]
    fn garbage_bytes_surface_an_error() {
        let err = run_scalar(&ImageInfo, &[Some(b"not an image")], Arguments::default());
        assert!(err.is_err(), "garbage bytes must error");
    }

    #[test]
    fn empty_blob_errors() {
        assert!(run_scalar(&ImageInfo, &[Some(b"")], Arguments::default()).is_err());
    }

    #[test]
    fn truncated_image_errors() {
        // First 40 bytes of a real PNG: header present, pixel data missing.
        let png = make_png(16, 16);
        let truncated = &png[..40.min(png.len())];
        assert!(run_scalar(&ImageInfo, &[Some(truncated)], Arguments::default()).is_err());
    }
}
