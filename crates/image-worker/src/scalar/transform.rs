//! `thumbnail(blob)` (128×128 JPEG default), `thumbnail_fit(blob, width, height,
//! format)` (explicit box + format), and `convert(blob, format)` — each decodes
//! the BLOB and emits a re-encoded BLOB.

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

/// Validate a bind-time dimension constant: a positive integer in `u32` range.
fn parse_dim(n: i64, name: &str) -> Result<u32> {
    if n > 0 && n <= i64::from(u32::MAX) {
        Ok(n as u32)
    } else {
        Err(RpcError::value_error(format!(
            "thumbnail_fit: '{name}' must be a positive integer, got {n}"
        )))
    }
}

/// Read the positional const dimension at index `pos` (`width`/`height`),
/// defaulting to 128 when the caller omits it.
fn dim_at(args: &Arguments, pos: usize, name: &str) -> Result<u32> {
    match args.const_i64(pos) {
        None => Ok(DEFAULT_DIM),
        Some(n) => parse_dim(n, name),
    }
}

pub struct Thumbnail;

impl ScalarFunction for Thumbnail {
    fn name(&self) -> &str {
        "thumbnail"
    }

    fn metadata(&self) -> FunctionMetadata {
        let example_sql = format!(
            "SELECT img.main.thumbnail({});",
            crate::meta::sample_png_expr()
        );
        let example_desc = "Generate a 128x128 aspect-preserving JPEG thumbnail of an image.";
        let mut tags = crate::meta::object_tags(
            "Generate Image Thumbnail",
            "Generate an aspect-preserving 128x128 JPEG thumbnail of an image `BLOB`, returned \
             as a `BLOB`. This is the one-argument convenience form; to choose the box size or \
             output format explicitly, use `thumbnail_fit(img, width, height, format)`. Returns \
             NULL for NULL input and errors on undecodable bytes. Use to build previews and \
             image galleries in SQL.",
            "Generate a 128x128 aspect-preserving JPEG thumbnail of an image `BLOB` (use \
             `thumbnail_fit` to control size and format).",
            "thumbnail, resize, downscale, preview, gallery, aspect ratio, re-encode, \
             jpeg, image transform",
            "transformation",
            "scalar/transform.rs",
        );
        tags.push(crate::meta::example_queries_tag(&[(
            example_desc,
            example_sql.clone(),
        )]));
        FunctionMetadata {
            description: "Generate a 128x128 aspect-preserving JPEG thumbnail of an image BLOB"
                .into(),
            return_type: Some(DataType::Binary),
            examples: vec![FunctionExample {
                sql: example_sql,
                description: example_desc.into(),
                expected_output: None,
            }],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![ArgSpec::any_column("blob", 0, "Image bytes (BLOB)")]
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Binary))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let fmt = OutFormat::parse("jpeg").map_err(|e| RpcError::value_error(e.to_string()))?;
        let col = batch.column(0);
        let rows = batch.num_rows();
        let mut b = BinaryBuilder::new();
        for i in 0..rows {
            match blob_bytes(col, i)? {
                None => b.append_null(),
                Some(bytes) => {
                    let out = imaging::thumbnail(bytes, DEFAULT_DIM, DEFAULT_DIM, fmt)
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

/// `thumbnail_fit(blob, width, height, format)` — thumbnail with an explicit
/// bounding box and output format. Width, height, and format are POSITIONAL
/// bind-time constants (DuckDB does not bind named arguments to scalar
/// functions), e.g. `thumbnail_fit(img, 64, 64, 'png')`.
pub struct ThumbnailFit;

impl ScalarFunction for ThumbnailFit {
    fn name(&self) -> &str {
        "thumbnail_fit"
    }

    fn metadata(&self) -> FunctionMetadata {
        let example_sql = format!(
            "SELECT img.main.thumbnail_fit({}, 64, 64, 'png');",
            crate::meta::sample_png_expr()
        );
        let example_desc =
            "Generate a 64x64 PNG thumbnail with an explicit box size and output format.";
        let mut tags = crate::meta::object_tags(
            "Generate Image Thumbnail (sized)",
            "Resize an image `BLOB` to fit within a `width` x `height` box (aspect-preserving) \
             and re-encode it to `format`, returning the thumbnail as a `BLOB`. `width`, \
             `height`, and `format` are POSITIONAL constants — `thumbnail_fit(img, 64, 64, \
             'png')` — because DuckDB does not bind named arguments to scalar functions. \
             `format` is one of jpeg, png, webp, gif, bmp, tiff. Returns NULL for NULL input \
             and errors on a non-positive dimension, an unknown format, or undecodable bytes. \
             Use `thumbnail(img)` for the 128x128 JPEG default.",
            "Resize an image `BLOB` to fit a `width` x `height` box and re-encode it to \
             `format` (jpeg, png, webp, gif, bmp, tiff); returns a thumbnail `BLOB`.",
            "thumbnail, resize, downscale, preview, gallery, aspect ratio, re-encode, \
             width, height, jpeg, png, webp, image transform",
            "transformation",
            "scalar/transform.rs",
        );
        tags.push(crate::meta::example_queries_tag(&[(
            example_desc,
            example_sql.clone(),
        )]));
        FunctionMetadata {
            description:
                "Resize an image BLOB to fit width×height (aspect-preserving) and re-encode to a format"
                    .into(),
            return_type: Some(DataType::Binary),
            examples: vec![FunctionExample {
                sql: example_sql,
                description: example_desc.into(),
                expected_output: None,
            }],
            tags,
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![
            ArgSpec::any_column("blob", 0, "Image bytes (BLOB)"),
            ArgSpec::const_arg(
                "width",
                1,
                "int64",
                "Maximum thumbnail width in pixels (must be positive)",
            )
            .with_ge(1.0),
            ArgSpec::const_arg(
                "height",
                2,
                "int64",
                "Maximum thumbnail height in pixels (must be positive)",
            )
            .with_ge(1.0),
            ArgSpec::const_arg(
                "format",
                3,
                "varchar",
                "Output format: jpeg, png, webp, gif, bmp, tiff",
            )
            // Closed set sourced from the decoder's own accepted spellings so the
            // discovery-facing constraint can never drift from behaviour.
            .with_choices(OutFormat::ACCEPTED.iter().copied()),
        ]
    }

    fn on_bind(&self, params: &BindParams) -> Result<BindResponse> {
        // Validate the constants eagerly (fail fast at bind).
        if let Some(n) = params.arguments.const_i64(1) {
            parse_dim(n, "width")?;
        }
        if let Some(n) = params.arguments.const_i64(2) {
            parse_dim(n, "height")?;
        }
        let s = params.arguments.const_str(3).ok_or_else(|| {
            RpcError::value_error("thumbnail_fit: a target format string is required")
        })?;
        OutFormat::parse(&s).map_err(|e| RpcError::value_error(e.to_string()))?;
        Ok(BindResponse::result(DataType::Binary))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let w = dim_at(&params.arguments, 1, "width")?;
        let h = dim_at(&params.arguments, 2, "height")?;
        let s = params.arguments.const_str(3).ok_or_else(|| {
            RpcError::value_error("thumbnail_fit: a target format string is required")
        })?;
        let fmt = OutFormat::parse(&s).map_err(|e| RpcError::value_error(e.to_string()))?;

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
        let example_sql = format!(
            "SELECT img.main.convert({}, 'png');",
            crate::meta::sample_png_expr()
        );
        let example_desc = "Convert an image BLOB to PNG at full resolution.";
        let mut tags = crate::meta::object_tags(
            "Convert Image Format",
            "Decode an image `BLOB` and re-encode it to another format at full resolution, \
             returning the converted `BLOB`. Target formats: jpeg, png, webp, gif, bmp, tiff. \
             Returns NULL for NULL input and errors on an unknown target format or \
             undecodable bytes. Use to normalize a mixed set of images to one format in SQL.",
            "Decode an image `BLOB` and re-encode it to another format (jpeg, png, webp, gif, \
             bmp, tiff) at full resolution.",
            "convert, transcode, re-encode, format conversion, png, jpeg, webp, gif, bmp, \
             tiff, change format, normalize images",
            "transformation",
            "scalar/transform.rs",
        );
        tags.push(crate::meta::example_queries_tag(&[(
            example_desc,
            example_sql.clone(),
        )]));
        FunctionMetadata {
            description:
                "Decode an image BLOB and re-encode it to another format (full resolution)".into(),
            return_type: Some(DataType::Binary),
            examples: vec![FunctionExample {
                sql: example_sql,
                description: example_desc.into(),
                expected_output: None,
            }],
            tags,
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
            )
            // Closed set sourced from the decoder's own accepted spellings so the
            // discovery-facing constraint can never drift from behaviour.
            .with_choices(OutFormat::ACCEPTED.iter().copied()),
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
    use arrow_array::{Array, Int64Array, StringArray};
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

    /// Build the positional const-arguments blob `(blob, width, height, format)`
    /// that DuckDB would hand `thumbnail_fit`.
    fn thumbnail_fit_args(width: i64, height: i64, format: &str) -> Arguments {
        let blob: ArrayRef = Arc::new(StringArray::from(vec![Some("blob")]));
        let w: ArrayRef = Arc::new(Int64Array::from(vec![width]));
        let h: ArrayRef = Arc::new(Int64Array::from(vec![height]));
        let fmt: ArrayRef = Arc::new(StringArray::from(vec![Some(format)]));
        let bytes = Arguments::serialize_positional(&[blob, w, h, fmt]).unwrap();
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

    #[test]
    fn thumbnail_fit_honors_size_and_format() {
        let png = make_png(160, 120);
        let out = run_scalar(
            &ThumbnailFit,
            &[Some(&png)],
            thumbnail_fit_args(64, 64, "png"),
        )
        .unwrap();
        let bytes = out.as_binary::<i32>().value(0);
        let info = imaging::image_info(bytes).unwrap();
        assert_eq!(
            info.format, "png",
            "output format must be the requested one"
        );
        assert!(
            info.width <= 64 && info.height <= 64,
            "thumbnail_fit must fit within the requested 64x64 box"
        );
    }

    #[test]
    fn thumbnail_fit_bad_format_errors_at_bind() {
        let bind = vgi::BindParams {
            arguments: thumbnail_fit_args(64, 64, "xyz"),
            ..Default::default()
        };
        assert!(ThumbnailFit.on_bind(&bind).is_err());
    }

    #[test]
    fn thumbnail_fit_non_positive_dimension_errors_at_bind() {
        let bind = vgi::BindParams {
            arguments: thumbnail_fit_args(0, 64, "png"),
            ..Default::default()
        };
        assert!(ThumbnailFit.on_bind(&bind).is_err());
    }

    #[test]
    fn thumbnail_fit_null_and_garbage() {
        let png = make_png(48, 48);
        let out = run_scalar(
            &ThumbnailFit,
            &[Some(&png), None],
            thumbnail_fit_args(32, 32, "bmp"),
        )
        .unwrap();
        assert!(!out.is_null(0));
        assert!(out.is_null(1));
        assert!(run_scalar(
            &ThumbnailFit,
            &[Some(b"")],
            thumbnail_fit_args(32, 32, "png")
        )
        .is_err());
        assert!(run_scalar(
            &ThumbnailFit,
            &[Some(b"nope")],
            thumbnail_fit_args(32, 32, "png")
        )
        .is_err());
    }
}
