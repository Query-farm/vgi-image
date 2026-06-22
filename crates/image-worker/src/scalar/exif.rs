//! `exif(blob)` → `MAP(VARCHAR, VARCHAR)` of flattened EXIF tags, and
//! `exif_gps(blob)` → `STRUCT(lat DOUBLE, lon DOUBLE)` (NULL if absent).

use std::sync::Arc;

use arrow_array::builder::Float64Builder;
use arrow_array::{ArrayRef, RecordBatch, StructArray};
use arrow_buffer::NullBuffer;
use arrow_schema::{DataType, Field, Fields};
use vgi::{ArgSpec, BindParams, BindResponse, FunctionMetadata, ProcessParams, ScalarFunction};
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
        FunctionMetadata {
            description: "Extract EXIF metadata from an image BLOB as a MAP(VARCHAR, VARCHAR)"
                .into(),
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
        FunctionMetadata {
            description:
                "Extract decimal GPS lat/lon from EXIF as a STRUCT(lat, lon); NULL if absent".into(),
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
