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
