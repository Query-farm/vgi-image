//! `exif(blob)` → `MAP(VARCHAR, VARCHAR)` of flattened EXIF tags, and
//! `exif_gps(blob)` → `STRUCT(lat DOUBLE, lon DOUBLE)` (NULL if absent).

use std::sync::Arc;

use arrow_array::builder::Float64Builder;
use arrow_array::{ArrayRef, RecordBatch, StructArray};
use arrow_buffer::NullBuffer;
use arrow_schema::{DataType, Field, Fields};
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::{
    append_map_null, append_map_row, blob_bytes, finish_map, map_builder, map_varchar_varchar_type,
};
use crate::imaging;

pub struct Exif;

impl ScalarFunction for Exif {
    fn name(&self) -> &str {
        "exif"
    }

    fn metadata(&self) -> FunctionMetadata {
        let example_sql = format!(
            "SELECT img.main.exif({})['Make'] AS camera_make;",
            crate::meta::sample_png_expr()
        );
        let example_desc = "Read the camera make from an image's EXIF metadata map.";
        let mut tags = crate::meta::object_tags(
            "Extract EXIF Metadata Map",
            "Extract EXIF metadata from an image `BLOB` as a `MAP(VARCHAR, VARCHAR)` of \
             flattened tag name to string value, e.g. camera Make/Model, exposure, ISO, \
             orientation, and timestamps. Images without EXIF yield an empty (non-null) \
             map; a NULL input yields a NULL map. Use to read camera and capture metadata \
             in SQL.",
            "Extract EXIF metadata from an image `BLOB` as a `MAP(VARCHAR, VARCHAR)`; index it \
             like `exif(blob)['Make']`.",
            "exif, metadata, camera, make, model, lens, ISO, exposure, orientation, \
             timestamp, tags, map, photo metadata",
            "metadata",
            "scalar/exif.rs",
        );
        tags.push(crate::meta::example_queries_tag(&[(
            example_desc,
            example_sql.clone(),
        )]));
        FunctionMetadata {
            description: "Extract EXIF metadata from an image BLOB as a MAP(VARCHAR, VARCHAR)"
                .into(),
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
        Ok(BindResponse::result(map_varchar_varchar_type()))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let col = batch.column(0);
        let rows = batch.num_rows();
        let mut builder = map_builder();
        for i in 0..rows {
            match blob_bytes(col, i)? {
                None => append_map_null(&mut builder)?,
                Some(bytes) => {
                    // A blob with no EXIF yields an empty (non-null) map.
                    let pairs =
                        imaging::exif(bytes).map_err(|e| RpcError::value_error(e.to_string()))?;
                    append_map_row(&mut builder, &pairs)?;
                }
            }
        }
        let out = finish_map(builder);
        RecordBatch::try_new(params.output_schema.clone(), vec![out])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

pub struct ExifGps;

fn gps_fields() -> Fields {
    Fields::from(vec![
        Field::new("lat", DataType::Float64, true),
        Field::new("lon", DataType::Float64, true),
    ])
}

impl ScalarFunction for ExifGps {
    fn name(&self) -> &str {
        "exif_gps"
    }

    fn metadata(&self) -> FunctionMetadata {
        let example_sql = format!(
            "SELECT (img.main.exif_gps({})).lat;",
            crate::meta::sample_png_expr()
        );
        let example_desc = "Extract decimal latitude and longitude from a geotagged \
                            image's EXIF GPS block.";
        let mut tags = crate::meta::object_tags(
            "Extract EXIF GPS Coordinates",
            "Extract the decimal GPS latitude and longitude from an image's EXIF GPS block \
             as a `STRUCT(lat DOUBLE, lon DOUBLE)`. Returns a NULL struct when the image has \
             no GPS tags or input is NULL. Use to map and geo-filter geotagged photos.",
            "Extract decimal GPS coordinates from an image's EXIF as `STRUCT(lat, lon)`; \
             NULL when no geotag is present.",
            "gps, geotag, latitude, longitude, coordinates, location, exif gps, geolocation, \
             map photos, decimal degrees",
            "metadata",
            "scalar/exif.rs",
        );
        tags.push(crate::meta::example_queries_tag(&[(
            example_desc,
            example_sql.clone(),
        )]));
        FunctionMetadata {
            description:
                "Extract decimal GPS lat/lon from EXIF as a STRUCT(lat, lon); NULL if absent".into(),
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
        Ok(BindResponse::result(DataType::Struct(gps_fields())))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let col = batch.column(0);
        let rows = batch.num_rows();
        let mut lat = Float64Builder::new();
        let mut lon = Float64Builder::new();
        let mut valid: Vec<bool> = Vec::with_capacity(rows);
        for i in 0..rows {
            let gps = match blob_bytes(col, i)? {
                None => None,
                Some(bytes) => {
                    imaging::exif_gps(bytes).map_err(|e| RpcError::value_error(e.to_string()))?
                }
            };
            match gps {
                Some(g) => {
                    lat.append_value(g.lat);
                    lon.append_value(g.lon);
                    valid.push(true);
                }
                None => {
                    lat.append_null();
                    lon.append_null();
                    valid.push(false);
                }
            }
        }
        let arrays: Vec<ArrayRef> = vec![Arc::new(lat.finish()), Arc::new(lon.finish())];
        let out: ArrayRef = Arc::new(StructArray::new(
            gps_fields(),
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
    use crate::arrow_io::map_varchar_varchar_type;
    use crate::arrow_io::test_support::{bound_type, make_png, run_scalar};
    use arrow_array::cast::AsArray;
    use arrow_array::Array;
    use vgi::arguments::Arguments;

    #[test]
    fn exif_bind_matches_built_map_type() {
        // The declared MAP DataType must equal what the MapBuilder produces.
        assert_eq!(bound_type(&Exif), map_varchar_varchar_type());
    }

    #[test]
    fn exif_gps_bind_declares_struct() {
        assert_eq!(bound_type(&ExifGps), DataType::Struct(gps_fields()));
    }

    #[test]
    fn exif_on_png_is_empty_map_not_null() {
        let png = make_png(16, 16);
        let out = run_scalar(&Exif, &[Some(&png)], Arguments::default()).unwrap();
        assert_eq!(out.data_type(), &map_varchar_varchar_type());
        let m = out.as_map();
        assert!(!m.is_null(0), "no-EXIF input → empty (non-null) map");
        assert_eq!(m.value(0).len(), 0);
    }

    #[test]
    fn exif_null_element_is_null_map() {
        let png = make_png(8, 8);
        let out = run_scalar(&Exif, &[Some(&png), None], Arguments::default()).unwrap();
        let m = out.as_map();
        assert!(!m.is_null(0));
        assert!(m.is_null(1), "NULL input → NULL map row");
    }

    #[test]
    fn exif_garbage_is_empty_map_not_error() {
        // EXIF parse is best-effort: non-image bytes simply carry no EXIF.
        let out = run_scalar(&Exif, &[Some(b"not an image")], Arguments::default()).unwrap();
        let m = out.as_map();
        assert!(!m.is_null(0));
        assert_eq!(m.value(0).len(), 0);
    }

    #[test]
    fn exif_gps_absent_is_null_struct() {
        let png = make_png(16, 16);
        let out = run_scalar(&ExifGps, &[Some(&png), None], Arguments::default()).unwrap();
        assert_eq!(out.data_type(), &DataType::Struct(gps_fields()));
        assert!(out.is_null(0), "no GPS → NULL struct");
        assert!(out.is_null(1), "NULL input → NULL struct");
    }
}
