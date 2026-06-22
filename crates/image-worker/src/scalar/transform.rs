//! `thumbnail(blob, width := 128, height := 128, format := 'jpeg')` and
//! `convert(blob, format)` — both decode the BLOB and emit a re-encoded BLOB.

use std::sync::Arc;

use arrow_array::builder::BinaryBuilder;
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::DataType;
use vgi::arguments::Arguments;
use vgi::{ArgSpec, BindParams, BindResponse, FunctionMetadata, ProcessParams, ScalarFunction};
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::blob_bytes;
use crate::imaging::{self, OutFormat};

const DEFAULT_DIM: u32 = 128;

fn dim_arg(args: &Arguments, name: &str) -> Result<u32> {
    match args.named_i64(name) {
        None => Ok(DEFAULT_DIM),
        Some(n) if n > 0 && n <= i64::from(u32::MAX) => Ok(n as u32),
        Some(n) => Err(RpcError::value_error(format!(
            "thumbnail: '{name}' must be a positive integer, got {n}"
        ))),
    }
}

fn format_arg(args: &Arguments, default: &str) -> Result<OutFormat> {
    let s = args
        .named_str("format")
        .unwrap_or_else(|| default.to_string());
    OutFormat::parse(&s).map_err(|e| RpcError::value_error(e.to_string()))
}

pub struct Thumbnail;

impl ScalarFunction for Thumbnail {
    fn name(&self) -> &str {
        "thumbnail"
    }

    fn metadata(&self) -> FunctionMetadata {
        FunctionMetadata {
            description:
                "Resize an image BLOB to fit width×height (aspect-preserving) and re-encode".into(),
            return_type: Some(DataType::Binary),
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![
            ArgSpec::any_column("blob", 0, "Image bytes (BLOB)"),
            ArgSpec::const_arg("width", -1, "int64", "Max width in pixels (default 128)"),
            ArgSpec::const_arg("height", -1, "int64", "Max height in pixels (default 128)"),
            ArgSpec::const_arg(
                "format",
                -1,
                "varchar",
                "Output format: jpeg (default), png, webp, gif, bmp, tiff",
            ),
        ]
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Binary))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let w = dim_arg(&params.arguments, "width")?;
        let h = dim_arg(&params.arguments, "height")?;
        let fmt = format_arg(&params.arguments, "jpeg")?;

        let col = batch.column(0);
        let rows = batch.num_rows();
        let mut b = BinaryBuilder::new();
        for i in 0..rows {
            match blob_bytes(col, i)? {
                None => b.append_null(),
                Some(bytes) => {
                    let out = imaging::thumbnail(bytes, w, h, fmt)
                        .map_err(|e| RpcError::value_error(e.to_string()))?;
                    b.append_value(&out);
                }
            }
        }
        let arr: ArrayRef = Arc::new(b.finish());
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

pub struct Convert;

impl ScalarFunction for Convert {
    fn name(&self) -> &str {
        "convert"
    }

    fn metadata(&self) -> FunctionMetadata {
        FunctionMetadata {
            description:
                "Decode an image BLOB and re-encode it to another format (full resolution)".into(),
            return_type: Some(DataType::Binary),
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![
            ArgSpec::any_column("blob", 0, "Image bytes (BLOB)"),
            ArgSpec::const_arg(
                "format",
                1,
                "varchar",
                "Target format: jpeg, png, webp, gif, bmp, tiff",
            ),
        ]
    }

    fn on_bind(&self, params: &BindParams) -> Result<BindResponse> {
        // Validate the format constant eagerly (fail fast at bind).
        let s = params
            .arguments
            .const_str(1)
            .ok_or_else(|| RpcError::value_error("convert: a target format string is required"))?;
        OutFormat::parse(&s).map_err(|e| RpcError::value_error(e.to_string()))?;
        Ok(BindResponse::result(DataType::Binary))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let s = params
            .arguments
            .const_str(1)
            .ok_or_else(|| RpcError::value_error("convert: a target format string is required"))?;
        let fmt = OutFormat::parse(&s).map_err(|e| RpcError::value_error(e.to_string()))?;

        let col = batch.column(0);
        let rows = batch.num_rows();
        let mut b = BinaryBuilder::new();
        for i in 0..rows {
            match blob_bytes(col, i)? {
                None => b.append_null(),
                Some(bytes) => {
                    let out = imaging::convert(bytes, fmt)
                        .map_err(|e| RpcError::value_error(e.to_string()))?;
                    b.append_value(&out);
                }
            }
        }
        let arr: ArrayRef = Arc::new(b.finish());
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}
