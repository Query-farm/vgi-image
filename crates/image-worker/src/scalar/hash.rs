//! Perceptual hashing: `phash` / `dhash` / `ahash` over an image BLOB → `UBIGINT`
//! (a packed 64-bit hash), and `phash_distance(a, b)` → `INT` Hamming distance.

use std::sync::Arc;

use arrow_array::builder::{Int32Builder, UInt64Builder};
use arrow_array::cast::AsArray;
use arrow_array::types::{
    Int16Type, Int32Type, Int64Type, Int8Type, UInt16Type, UInt32Type, UInt64Type, UInt8Type,
};
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_schema::DataType;
use vgi::{ArgSpec, BindParams, BindResponse, FunctionMetadata, ProcessParams, ScalarFunction};
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::blob_bytes;
use crate::imaging::{self, HashKind};

/// One of the three perceptual-hash scalar functions. The same struct backs all
/// three; `name`/`kind` differ.
pub struct PerceptualHash {
    name: &'static str,
    kind: HashKind,
    description: &'static str,
}

impl PerceptualHash {
    pub fn phash() -> Self {
        PerceptualHash {
            name: "phash",
            kind: HashKind::Perceptual,
            description: "64-bit DCT perceptual hash of an image BLOB (UBIGINT)",
        }
    }
    pub fn dhash() -> Self {
        PerceptualHash {
            name: "dhash",
            kind: HashKind::Difference,
            description: "64-bit difference (gradient) hash of an image BLOB (UBIGINT)",
        }
    }
    pub fn ahash() -> Self {
        PerceptualHash {
            name: "ahash",
            kind: HashKind::Average,
            description: "64-bit average hash of an image BLOB (UBIGINT)",
        }
    }
}

impl ScalarFunction for PerceptualHash {
    fn name(&self) -> &str {
        self.name
    }

    fn metadata(&self) -> FunctionMetadata {
        FunctionMetadata {
            description: self.description.into(),
            return_type: Some(DataType::UInt64),
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![ArgSpec::any_column("blob", 0, "Image bytes (BLOB)")]
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::UInt64))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let col = batch.column(0);
        let rows = batch.num_rows();
        let mut b = UInt64Builder::new();
        for i in 0..rows {
            match blob_bytes(col, i)? {
                None => b.append_null(),
                Some(bytes) => {
                    let h = imaging::perceptual_hash(bytes, self.kind)
                        .map_err(|e| RpcError::value_error(e.to_string()))?;
                    b.append_value(h);
                }
            }
        }
        let out: ArrayRef = Arc::new(b.finish());
        RecordBatch::try_new(params.output_schema.clone(), vec![out])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}

/// `phash_distance(a, b)` — Hamming distance between two packed 64-bit hashes.
/// A pure integer scalar; inputs are taken as any unsigned/signed integer and
/// reinterpreted as the underlying 64 hash bits.
pub struct PhashDistance;

/// Read element `row` of an integer array as the raw `u64` hash bits.
fn hash_bits(col: &ArrayRef, row: usize) -> Result<Option<u64>> {
    if col.is_null(row) {
        return Ok(None);
    }
    Ok(Some(match col.data_type() {
        DataType::UInt64 => col.as_primitive::<UInt64Type>().value(row),
        DataType::Int64 => col.as_primitive::<Int64Type>().value(row) as u64,
        DataType::UInt32 => col.as_primitive::<UInt32Type>().value(row) as u64,
        DataType::Int32 => col.as_primitive::<Int32Type>().value(row) as u64,
        DataType::UInt16 => col.as_primitive::<UInt16Type>().value(row) as u64,
        DataType::Int16 => col.as_primitive::<Int16Type>().value(row) as u64,
        DataType::UInt8 => col.as_primitive::<UInt8Type>().value(row) as u64,
        DataType::Int8 => col.as_primitive::<Int8Type>().value(row) as u64,
        other => {
            return Err(RpcError::value_error(format!(
                "phash_distance: arguments must be integer hashes, got {other:?}"
            )))
        }
    }))
}

impl ScalarFunction for PhashDistance {
    fn name(&self) -> &str {
        "phash_distance"
    }

    fn metadata(&self) -> FunctionMetadata {
        FunctionMetadata {
            description: "Hamming distance (0-64) between two packed 64-bit perceptual hashes"
                .into(),
            return_type: Some(DataType::Int32),
            ..Default::default()
        }
    }

    fn argument_specs(&self) -> Vec<ArgSpec> {
        vec![
            ArgSpec::any_column("a", 0, "First hash (UBIGINT)"),
            ArgSpec::any_column("b", 1, "Second hash (UBIGINT)"),
        ]
    }

    fn on_bind(&self, _params: &BindParams) -> Result<BindResponse> {
        Ok(BindResponse::result(DataType::Int32))
    }

    fn process(&self, params: &ProcessParams, batch: &RecordBatch) -> Result<RecordBatch> {
        let a = batch.column(0);
        let b = batch.column(1);
        let rows = batch.num_rows();
        let mut out = Int32Builder::new();
        for i in 0..rows {
            match (hash_bits(a, i)?, hash_bits(b, i)?) {
                (Some(x), Some(y)) => out.append_value(imaging::hamming_distance(x, y) as i32),
                _ => out.append_null(),
            }
        }
        let arr: ArrayRef = Arc::new(out.finish());
        RecordBatch::try_new(params.output_schema.clone(), vec![arr])
            .map_err(|e| RpcError::runtime_error(e.to_string()))
    }
}
