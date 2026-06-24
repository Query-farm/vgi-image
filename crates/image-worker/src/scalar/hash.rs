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
use vgi::{
    ArgSpec, BindParams, BindResponse, FunctionExample, FunctionMetadata, ProcessParams,
    ScalarFunction,
};
use vgi_rpc::{Result, RpcError};

use crate::arrow_io::blob_bytes;
use crate::imaging::{self, HashKind};

/// One of the three perceptual-hash scalar functions. The same struct backs all
/// three; `name`/`kind` differ.
pub struct PerceptualHash {
    name: &'static str,
    kind: HashKind,
    description: &'static str,
    example_sql: &'static str,
    example_desc: &'static str,
}

impl PerceptualHash {
    pub fn phash() -> Self {
        PerceptualHash {
            name: "phash",
            kind: HashKind::Perceptual,
            description: "64-bit DCT perceptual hash of an image BLOB (UBIGINT)",
            example_sql: "SELECT img.main.phash(read_blob('photo.jpg'));",
            example_desc: "Compute the 64-bit DCT perceptual hash of an image for \
                           near-duplicate detection.",
        }
    }
    pub fn dhash() -> Self {
        PerceptualHash {
            name: "dhash",
            kind: HashKind::Difference,
            description: "64-bit difference (gradient) hash of an image BLOB (UBIGINT)",
            example_sql: "SELECT img.main.dhash(read_blob('photo.jpg'));",
            example_desc: "Compute the 64-bit difference (gradient) hash of an image.",
        }
    }
    pub fn ahash() -> Self {
        PerceptualHash {
            name: "ahash",
            kind: HashKind::Average,
            description: "64-bit average hash of an image BLOB (UBIGINT)",
            example_sql: "SELECT img.main.ahash(read_blob('photo.jpg'));",
            example_desc: "Compute the 64-bit average hash of an image.",
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
            examples: vec![FunctionExample {
                sql: self.example_sql.into(),
                description: self.example_desc.into(),
                expected_output: None,
            }],
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
            examples: vec![FunctionExample {
                sql: "SELECT img.main.phash_distance(\
                      img.main.phash(read_blob('a.jpg')), \
                      img.main.phash(read_blob('b.jpg'))) AS distance;"
                    .into(),
                description: "Measure how similar two images are by the Hamming distance \
                              between their perceptual hashes (0 = identical)."
                    .into(),
                expected_output: None,
            }],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrow_io::test_support::{
        blob_batch, bound_type, make_png, process_params, run_scalar,
    };
    use arrow_array::builder::UInt64Builder;
    use arrow_array::cast::AsArray;
    use arrow_array::types::UInt64Type;
    use vgi::arguments::Arguments;
    use vgi::{BindParams, ScalarFunction};

    #[test]
    fn bind_declares_ubigint() {
        for f in [
            PerceptualHash::phash(),
            PerceptualHash::dhash(),
            PerceptualHash::ahash(),
        ] {
            assert_eq!(bound_type(&f), DataType::UInt64);
        }
        assert_eq!(bound_type(&PhashDistance), DataType::Int32);
    }

    #[test]
    fn phash_of_png_is_deterministic_and_u64() {
        let png = make_png(40, 40);
        let h1 = run_scalar(
            &PerceptualHash::phash(),
            &[Some(&png)],
            Arguments::default(),
        )
        .unwrap();
        let h2 = run_scalar(
            &PerceptualHash::phash(),
            &[Some(&png)],
            Arguments::default(),
        )
        .unwrap();
        assert_eq!(h1.data_type(), &DataType::UInt64);
        let a = h1.as_primitive::<UInt64Type>().value(0);
        let b = h2.as_primitive::<UInt64Type>().value(0);
        assert_eq!(a, b, "phash must be deterministic across calls");
    }

    #[test]
    fn null_and_garbage_handling() {
        let png = make_png(16, 16);
        // NULL element → NULL hash, valid element alongside still works.
        let out = run_scalar(
            &PerceptualHash::phash(),
            &[Some(&png), None],
            Arguments::default(),
        )
        .unwrap();
        assert!(!out.is_null(0));
        assert!(out.is_null(1));
        // Garbage / empty / truncated → error.
        for bad in [&b""[..], &b"nope"[..], &png[..30.min(png.len())]] {
            assert!(
                run_scalar(&PerceptualHash::phash(), &[Some(bad)], Arguments::default()).is_err(),
                "expected error for {} bytes",
                bad.len()
            );
        }
    }

    /// `phash_distance` takes two integer columns; build them directly and run.
    fn distance(a: &[Option<u64>], b: &[Option<u64>]) -> ArrayRef {
        use arrow_array::RecordBatch;
        use arrow_schema::{Field, Schema};
        let mk = |vals: &[Option<u64>]| {
            let mut x = UInt64Builder::new();
            for v in vals {
                match v {
                    Some(n) => x.append_value(*n),
                    None => x.append_null(),
                }
            }
            Arc::new(x.finish()) as ArrayRef
        };
        let ca = mk(a);
        let cb = mk(b);
        let schema = Arc::new(Schema::new(vec![
            Field::new("a", DataType::UInt64, true),
            Field::new("b", DataType::UInt64, true),
        ]));
        let batch = RecordBatch::try_new(schema.clone(), vec![ca, cb]).unwrap();
        let bind = BindParams {
            input_schema: Some(schema),
            ..Default::default()
        };
        let bound = PhashDistance.on_bind(&bind).unwrap();
        let params = process_params(bound.output_schema, Arguments::default());
        PhashDistance
            .process(&params, &batch)
            .unwrap()
            .column(0)
            .clone()
    }

    #[test]
    fn distance_basics_and_nulls() {
        let out = distance(
            &[Some(0), Some(0b1011), Some(u64::MAX), Some(7), None],
            &[Some(0), Some(0b0001), Some(0), None, Some(0)],
        );
        let d = out.as_primitive::<arrow_array::types::Int32Type>();
        assert_eq!(d.value(0), 0);
        assert_eq!(d.value(1), 2);
        assert_eq!(d.value(2), 64);
        assert!(out.is_null(3), "NULL operand → NULL distance");
        assert!(out.is_null(4));
    }

    #[test]
    fn distance_rejects_non_integer_arg() {
        // A Utf8 input column is not an integer hash → error.
        use arrow_array::RecordBatch;
        use arrow_array::StringArray;
        use arrow_schema::{Field, Schema};
        let a: ArrayRef = Arc::new(StringArray::from(vec!["x"]));
        let b: ArrayRef = blob_batch(&[Some(b"")]).column(0).clone();
        let schema = Arc::new(Schema::new(vec![
            Field::new("a", a.data_type().clone(), true),
            Field::new("b", b.data_type().clone(), true),
        ]));
        let batch = RecordBatch::try_new(schema, vec![a, b]).unwrap();
        let params = process_params(
            PhashDistance
                .on_bind(&BindParams::default())
                .unwrap()
                .output_schema,
            Arguments::default(),
        );
        assert!(PhashDistance.process(&params, &batch).is_err());
    }
}
