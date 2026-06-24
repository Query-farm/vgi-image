//! `thumbnail(blob, width := 128, height := 128, format := 'jpeg')` and
//! `convert(blob, format)` — both decode the BLOB and emit a re-encoded BLOB.

use std::sync::Arc;

use arrow_array::builder::BinaryBuilder;
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::DataType;
use vgi::arguments::Arguments;
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
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
            examples: vec![FunctionExample {
                sql: "SELECT img.main.thumbnail(read_blob('photo.jpg'));".into(),
                description: "Generate a 128×128 aspect-preserving JPEG thumbnail of an image."
                    .into(),
                expected_output: None,
            }],
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
            examples: vec![FunctionExample {
                sql: "SELECT img.main.convert(read_blob('photo.jpg'), 'png');".into(),
                description: "Convert a JPEG image to PNG at full resolution.".into(),
                expected_output: None,
            }],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow_io::test_support::{bound_type, make_png, run_scalar};
    use crate::imaging;
    use arrow_array::cast::AsArray;
    use arrow_array::{Array, StringArray};
    use vgi::arguments::Arguments;

    /// Build the positional const-arguments blob `(blob, format)` that DuckDB
    /// would hand `convert` (positional_0 = blob placeholder, positional_1 =
    /// the format string).
    fn convert_args(format: &str) -> Arguments {
        let blob: ArrayRef = Arc::new(StringArray::from(vec![Some("blob")]));
        let fmt: ArrayRef = Arc::new(StringArray::from(vec![Some(format)]));
        let bytes = Arguments::serialize_positional(&[blob, fmt]).unwrap();
        Arguments::parse(&bytes).unwrap()
    }

    #[test]
    fn thumbnail_and_convert_bind_binary() {
        // Thumbnail binds with no required args.
        assert_eq!(bound_type(&Thumbnail), DataType::Binary);
        // Convert validates its format constant at bind, so it needs the args.
        let bind = vgi::BindParams {
            arguments: convert_args("png"),
            ..Default::default()
        };
        let bound = Convert.on_bind(&bind).unwrap();
        assert_eq!(bound.output_schema.field(0).data_type(), &DataType::Binary);
    }

    #[test]
    fn thumbnail_produces_a_smaller_valid_image() {
        let png = make_png(160, 120);
        let out = run_scalar(&Thumbnail, &[Some(&png)], Arguments::default()).unwrap();
        assert_eq!(out.data_type(), &DataType::Binary);
        let bytes = out.as_binary::<i32>().value(0);
        // Re-feed the produced BLOB into the decoder: it must be a real image
        // that fits within the default 128x128 box (default format is jpeg).
        let info = imaging::image_info(bytes).unwrap();
        assert_eq!(info.format, "jpeg");
        assert!(info.width <= 128 && info.height <= 128);
        assert!(info.width < 160, "thumbnail should have shrunk the source");
    }

    #[test]
    fn thumbnail_null_and_garbage() {
        let png = make_png(32, 32);
        let out = run_scalar(&Thumbnail, &[Some(&png), None], Arguments::default()).unwrap();
        assert!(!out.is_null(0));
        assert!(out.is_null(1));
        assert!(run_scalar(&Thumbnail, &[Some(b"")], Arguments::default()).is_err());
        assert!(run_scalar(&Thumbnail, &[Some(b"nope")], Arguments::default()).is_err());
    }

    #[test]
    fn convert_changes_format_and_keeps_dimensions() {
        let png = make_png(40, 30);
        let out = run_scalar(&Convert, &[Some(&png)], convert_args("bmp")).unwrap();
        let bytes = out.as_binary::<i32>().value(0);
        let info = imaging::image_info(bytes).unwrap();
        assert_eq!(info.format, "bmp");
        assert_eq!((info.width, info.height), (40, 30));
    }

    #[test]
    fn convert_bad_format_errors_at_bind() {
        // on_bind validates the format constant eagerly.
        let bind = vgi::BindParams {
            arguments: convert_args("xyz"),
            ..Default::default()
        };
        assert!(Convert.on_bind(&bind).is_err());
    }

    #[test]
    fn convert_null_and_garbage() {
        let out = run_scalar(&Convert, &[None], convert_args("png")).unwrap();
        assert!(out.is_null(0));
        assert!(run_scalar(&Convert, &[Some(b"")], convert_args("png")).is_err());
    }
}
