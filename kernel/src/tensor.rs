//! Integer tensors. Every value is held as `i32` and wrapped to its type's width after each op: the same
//! arithmetic as WGSL (32-bit, two's complement), so a CPU run and a GPU run give the same bits.

use crate::proto::TensorProto;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DType {
    I8,
    U8,
    I16,
    U16,
    I32,
    /// ONNX int64 (indices, shapes, `ArgMax`). Values must fit in 32 bits: checked on load, wraps at run time.
    I64,
    Bool,
}

impl DType {
    pub fn from_onnx(code: i32) -> Result<DType, String> {
        Ok(match code {
            2 => DType::U8,
            3 => DType::I8,
            4 => DType::U16,
            5 => DType::I16,
            6 => DType::I32,
            7 => DType::I64,
            9 => DType::Bool,
            1 | 10 | 11 | 16..=20 | 23 | 24 | 27 | 28 => {
                return Err("float tensors give different bits on different devices; quantize the model to \
                            int8/int32 (weights int8, accumulators int32, rescale with Mul + Div)"
                    .into());
            }
            other => return Err(format!("tensor type {other} is not supported (int8, uint8, int16, uint16, int32, int64, bool)")),
        })
    }

    pub fn onnx(self) -> i32 {
        match self {
            DType::U8 => 2,
            DType::I8 => 3,
            DType::U16 => 4,
            DType::I16 => 5,
            DType::I32 => 6,
            DType::I64 => 7,
            DType::Bool => 9,
        }
    }

    /// Bytes per value in the ONNX file (what a model costs on disk and in the budget).
    pub fn width(self) -> usize {
        match self {
            DType::I8 | DType::U8 | DType::Bool => 1,
            DType::I16 | DType::U16 => 2,
            DType::I32 => 4,
            DType::I64 => 8,
        }
    }

    #[inline]
    pub fn wrap(self, v: i32) -> i32 {
        match self {
            DType::I8 => v as i8 as i32,
            DType::U8 => v as u8 as i32,
            DType::I16 => v as i16 as i32,
            DType::U16 => v as u16 as i32,
            DType::I32 | DType::I64 => v,
            DType::Bool => (v != 0) as i32,
        }
    }

    pub fn is_unsigned(self) -> bool {
        matches!(self, DType::U8 | DType::U16 | DType::Bool)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tensor {
    pub dtype: DType,
    pub shape: Vec<usize>,
    pub data: Vec<i32>,
}

impl Tensor {
    /// Values are wrapped to `dtype`.
    pub fn new(dtype: DType, shape: Vec<usize>, data: Vec<i32>) -> Result<Tensor, String> {
        let n: usize = shape.iter().product();
        if n != data.len() {
            return Err(format!("shape {shape:?} holds {n} values, got {}", data.len()));
        }
        Ok(Tensor { dtype, shape, data: data.into_iter().map(|v| dtype.wrap(v)).collect() })
    }

    pub fn scalar(dtype: DType, v: i32) -> Tensor {
        Tensor { dtype, shape: vec![], data: vec![dtype.wrap(v)] }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Bytes this tensor takes in an ONNX file.
    pub fn bytes(&self) -> usize {
        self.data.len() * self.dtype.width()
    }

    pub fn from_proto(t: &TensorProto) -> Result<Tensor, String> {
        let name = &t.name;
        if t.data_location == 1 {
            return Err(format!("tensor '{name}': external data; a model must be a single file"));
        }
        let dtype = DType::from_onnx(t.data_type).map_err(|e| format!("tensor '{name}': {e}"))?;
        let shape = t
            .dims
            .iter()
            .map(|d| usize::try_from(*d).map_err(|_| format!("tensor '{name}': negative dimension {d}")))
            .collect::<Result<Vec<_>, _>>()?;
        let n: usize = shape.iter().product();
        let data: Vec<i32> = if !t.raw_data.is_empty() {
            let w = dtype.width();
            if t.raw_data.len() != n * w {
                return Err(format!("tensor '{name}': raw data is {} bytes, shape {shape:?} needs {}", t.raw_data.len(), n * w));
            }
            let mut out = Vec::with_capacity(n);
            for c in t.raw_data.chunks_exact(w) {
                out.push(match dtype {
                    DType::I8 => c[0] as i8 as i32,
                    DType::U8 | DType::Bool => c[0] as i32,
                    DType::I16 => i16::from_le_bytes([c[0], c[1]]) as i32,
                    DType::U16 => u16::from_le_bytes([c[0], c[1]]) as i32,
                    DType::I32 => i32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                    DType::I64 => narrow(i64::from_le_bytes(c.try_into().expect("8 bytes")), name)?,
                });
            }
            out
        } else if dtype == DType::I64 {
            t.int64_data.iter().map(|v| narrow(*v, name)).collect::<Result<_, _>>()?
        } else {
            t.int32_data.clone()
        };
        Tensor::new(dtype, shape, data).map_err(|e| format!("tensor '{name}': {e}"))
    }

    pub fn to_proto(&self, name: &str) -> TensorProto {
        let mut raw = Vec::with_capacity(self.bytes());
        for &v in &self.data {
            match self.dtype.width() {
                1 => raw.push(v as u8),
                2 => raw.extend_from_slice(&(v as i16).to_le_bytes()),
                4 => raw.extend_from_slice(&v.to_le_bytes()),
                _ => raw.extend_from_slice(&(v as i64).to_le_bytes()),
            }
        }
        TensorProto {
            dims: self.shape.iter().map(|d| *d as i64).collect(),
            data_type: self.dtype.onnx(),
            name: name.to_string(),
            raw_data: raw,
            ..Default::default()
        }
    }
}

fn narrow(v: i64, name: &str) -> Result<i32, String> {
    i32::try_from(v).map_err(|_| format!("tensor '{name}': {v} does not fit in 32 bits (the kernel is a 32-bit machine)"))
}

/// Row-major strides.
pub fn strides(shape: &[usize]) -> Vec<usize> {
    let mut s = vec![1; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        s[i] = s[i + 1] * shape[i + 1];
    }
    s
}

/// Numpy-style broadcasting of two shapes.
pub fn broadcast(a: &[usize], b: &[usize]) -> Result<Vec<usize>, String> {
    let r = a.len().max(b.len());
    let dim = |s: &[usize], i: usize| if i + s.len() >= r { s[i + s.len() - r] } else { 1 };
    (0..r)
        .map(|i| match (dim(a, i), dim(b, i)) {
            (x, y) if x == y => Ok(x),
            (1, y) => Ok(y),
            (x, 1) => Ok(x),
            (x, y) => Err(format!("shapes {a:?} and {b:?} do not broadcast ({x} vs {y})")),
        })
        .collect()
}

/// For each output position, the index into a tensor of `shape` broadcast to `out`.
pub fn broadcast_index(shape: &[usize], out: &[usize]) -> Vec<usize> {
    let n: usize = out.iter().product();
    if shape == out {
        return (0..n).collect();
    }
    let st = strides(shape);
    let lead = out.len() - shape.len();
    // Stride per output dimension: 0 where the input is broadcast (size 1 or missing).
    let eff: Vec<usize> = (0..out.len()).map(|i| if i < lead || shape[i - lead] == 1 { 0 } else { st[i - lead] }).collect();
    let mut idx = vec![0usize; out.len()];
    let mut at = 0usize;
    let mut res = Vec::with_capacity(n);
    for _ in 0..n {
        res.push(at);
        for d in (0..out.len()).rev() {
            idx[d] += 1;
            at += eff[d];
            if idx[d] < out[d] {
                break;
            }
            at -= eff[d] * idx[d];
            idx[d] = 0;
        }
    }
    res
}
