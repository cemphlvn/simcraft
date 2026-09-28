//! The operators. Integer only; each one follows the ONNX operator spec, with 32-bit wrapping arithmetic
//! (as in WGSL) and one definition the spec leaves open: `x / 0 = x` (also WGSL's rule).

use crate::proto::AttributeProto;
use crate::tensor::{DType, Tensor, broadcast, broadcast_index, strides};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    And,
    Or,
    Xor,
    Equal,
    Less,
    Greater,
    LessOrEqual,
    GreaterOrEqual,
    ShiftLeft,
    ShiftRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Un {
    Identity,
    Neg,
    Abs,
    Sign,
    Relu,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Red {
    Sum,
    Max,
    Min,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Bin(Bin),
    /// Variadic `Min` / `Max` / `Sum` (folded left to right).
    Fold(Bin),
    Un(Un),
    Clip,
    Cast(DType),
    Where,
    MatMul,
    MatMulInteger,
    Gemm {
        trans_a: bool,
        trans_b: bool,
    },
    Arg {
        max: bool,
        axis: i64,
        keepdims: bool,
        last: bool,
    },
    Reduce {
        red: Red,
        axes: Option<Vec<i64>>,
        keepdims: bool,
        noop_empty: bool,
    },
    Reshape {
        allow_zero: bool,
    },
    Flatten {
        axis: i64,
    },
    Squeeze {
        axes: Option<Vec<i64>>,
    },
    Unsqueeze {
        axes: Option<Vec<i64>>,
    },
    Concat {
        axis: i64,
    },
    Gather {
        axis: i64,
    },
    Transpose {
        perm: Option<Vec<usize>>,
    },
}

pub const SUPPORTED: &str = "Add Sub Mul Div Min Max Sum And Or Xor Not Equal Less Greater LessOrEqual GreaterOrEqual \
    BitShift Identity Neg Abs Sign Relu Clip Cast Where MatMul MatMulInteger Gemm ArgMax ArgMin ReduceSum ReduceMax \
    ReduceMin Reshape Flatten Squeeze Unsqueeze Concat Gather Transpose Constant";

fn attr<'a>(attrs: &'a [AttributeProto], name: &str) -> Option<&'a AttributeProto> {
    attrs.iter().find(|a| a.name == name)
}

fn int(attrs: &[AttributeProto], name: &str, default: i64) -> i64 {
    attr(attrs, name).map_or(default, |a| a.i)
}

fn ints(attrs: &[AttributeProto], name: &str) -> Option<Vec<i64>> {
    attr(attrs, name).map(|a| a.ints.clone())
}

/// An op from its ONNX name and attributes (checked once, at load).
pub fn parse(op_type: &str, attrs: &[AttributeProto]) -> Result<Op, String> {
    let bin = |b| Ok(Op::Bin(b));
    match op_type {
        "Add" => bin(Bin::Add),
        "Sub" => bin(Bin::Sub),
        "Mul" => bin(Bin::Mul),
        "Div" => bin(Bin::Div),
        "And" => bin(Bin::And),
        "Or" => bin(Bin::Or),
        "Xor" => bin(Bin::Xor),
        "Equal" => bin(Bin::Equal),
        "Less" => bin(Bin::Less),
        "Greater" => bin(Bin::Greater),
        "LessOrEqual" => bin(Bin::LessOrEqual),
        "GreaterOrEqual" => bin(Bin::GreaterOrEqual),
        "BitShift" => match attr(attrs, "direction").map(|a| a.s.as_slice()) {
            Some(b"LEFT") => bin(Bin::ShiftLeft),
            Some(b"RIGHT") => bin(Bin::ShiftRight),
            _ => Err("BitShift needs direction LEFT or RIGHT".into()),
        },
        "Min" => Ok(Op::Fold(Bin::Min)),
        "Max" => Ok(Op::Fold(Bin::Max)),
        "Sum" => Ok(Op::Fold(Bin::Add)),
        "Identity" => Ok(Op::Un(Un::Identity)),
        "Neg" => Ok(Op::Un(Un::Neg)),
        "Abs" => Ok(Op::Un(Un::Abs)),
        "Sign" => Ok(Op::Un(Un::Sign)),
        "Relu" => Ok(Op::Un(Un::Relu)),
        "Not" => Ok(Op::Un(Un::Not)),
        "Clip" => Ok(Op::Clip),
        "Cast" => Ok(Op::Cast(DType::from_onnx(int(attrs, "to", 0) as i32)?)),
        "Where" => Ok(Op::Where),
        "MatMul" => Ok(Op::MatMul),
        "MatMulInteger" => Ok(Op::MatMulInteger),
        "Gemm" => {
            for k in ["alpha", "beta"] {
                if attr(attrs, k).is_some_and(|a| a.f != 1.0) {
                    return Err(format!("Gemm {k} must be 1 (a float scale is not deterministic); fold it into the weights"));
                }
            }
            Ok(Op::Gemm { trans_a: int(attrs, "transA", 0) != 0, trans_b: int(attrs, "transB", 0) != 0 })
        }
        "ArgMax" | "ArgMin" => Ok(Op::Arg {
            max: op_type == "ArgMax",
            axis: int(attrs, "axis", 0),
            keepdims: int(attrs, "keepdims", 1) != 0,
            last: int(attrs, "select_last_index", 0) != 0,
        }),
        "ReduceSum" | "ReduceMax" | "ReduceMin" => Ok(Op::Reduce {
            red: match op_type {
                "ReduceSum" => Red::Sum,
                "ReduceMax" => Red::Max,
                _ => Red::Min,
            },
            axes: ints(attrs, "axes"),
            keepdims: int(attrs, "keepdims", 1) != 0,
            noop_empty: int(attrs, "noop_with_empty_axes", 0) != 0,
        }),
        "Reshape" => Ok(Op::Reshape { allow_zero: int(attrs, "allowzero", 0) != 0 }),
        "Flatten" => Ok(Op::Flatten { axis: int(attrs, "axis", 1) }),
        "Squeeze" => Ok(Op::Squeeze { axes: ints(attrs, "axes") }),
        "Unsqueeze" => Ok(Op::Unsqueeze { axes: ints(attrs, "axes") }),
        "Concat" => Ok(Op::Concat { axis: int(attrs, "axis", 0) }),
        "Gather" => Ok(Op::Gather { axis: int(attrs, "axis", 0) }),
        "Transpose" => Ok(Op::Transpose { perm: ints(attrs, "perm").map(|p| p.into_iter().map(|v| v as usize).collect()) }),
        "QuantizeLinear" | "DequantizeLinear" | "QLinearMatMul" | "QLinearConv" => {
            Err(format!("{op_type} rescales with a float; export integer-only (MatMulInteger, then Mul + Div by a power of two)"))
        }
        other => Err(format!("operator '{other}' is not in the kernel (supported: {SUPPORTED})")),
    }
}

fn axis(a: i64, rank: usize) -> Result<usize, String> {
    let r = rank as i64;
    let v = if a < 0 { a + r } else { a };
    if (0..r.max(1)).contains(&v) { Ok(v as usize) } else { Err(format!("axis {a} out of range for rank {rank}")) }
}

fn need<'a>(ins: &[Option<&'a Tensor>], i: usize, op: &str) -> Result<&'a Tensor, String> {
    ins.get(i).copied().flatten().ok_or_else(|| format!("{op}: input {i} is missing"))
}

fn same(a: &Tensor, b: &Tensor, op: &str) -> Result<(), String> {
    if a.dtype == b.dtype { Ok(()) } else { Err(format!("{op}: inputs are {:?} and {:?}; Cast one first", a.dtype, b.dtype)) }
}

/// `f` over two tensors (broadcast), wrapped to `out_t`. Generic so each op compiles to its own tight loop.
fn zip_with<F: Fn(i32, i32) -> i32 + Copy>(x: &Tensor, y: &Tensor, out_t: DType, f: F) -> Result<Tensor, String> {
    match out_t {
        DType::I32 | DType::I64 => zip_raw(x, y, out_t, f),
        _ => zip_raw(x, y, out_t, move |p, q| out_t.wrap(f(p, q))),
    }
}

fn zip_raw<F: Fn(i32, i32) -> i32 + Copy>(x: &Tensor, y: &Tensor, out_t: DType, f: F) -> Result<Tensor, String> {
    if x.shape == y.shape {
        let data = x.data.iter().zip(&y.data).map(|(p, q)| f(*p, *q)).collect();
        return Ok(Tensor { dtype: out_t, shape: x.shape.clone(), data });
    }
    // Common shapes without index tables: a bias row ([n, k] + [k]) or a scalar, on either side.
    if !y.is_empty() && x.shape.ends_with(&y.shape) {
        let mut data = Vec::with_capacity(x.len());
        for c in x.data.chunks_exact(y.len()) {
            data.extend(c.iter().zip(&y.data).map(|(p, q)| f(*p, *q)));
        }
        return Ok(Tensor { dtype: out_t, shape: x.shape.clone(), data });
    }
    if !x.is_empty() && y.shape.ends_with(&x.shape) {
        let mut data = Vec::with_capacity(y.len());
        for c in y.data.chunks_exact(x.len()) {
            data.extend(x.data.iter().zip(c).map(|(p, q)| f(*p, *q)));
        }
        return Ok(Tensor { dtype: out_t, shape: y.shape.clone(), data });
    }
    let shape = broadcast(&x.shape, &y.shape)?;
    let (ix, iy) = (broadcast_index(&x.shape, &shape), broadcast_index(&y.shape, &shape));
    let data = ix.iter().zip(&iy).map(|(i, j)| f(x.data[*i], y.data[*j])).collect();
    Ok(Tensor { dtype: out_t, shape, data })
}

fn binary(b: Bin, x: &Tensor, y: &Tensor) -> Result<Tensor, String> {
    same(x, y, &format!("{b:?}"))?;
    if matches!(b, Bin::ShiftLeft | Bin::ShiftRight) && !x.dtype.is_unsigned() {
        return Err(format!("BitShift is for unsigned types (got {:?}); divide by a power of two instead", x.dtype));
    }
    let (t, bool_t) = (x.dtype, DType::Bool);
    match b {
        Bin::Add => zip_with(x, y, t, |p, q| p.wrapping_add(q)),
        Bin::Sub => zip_with(x, y, t, |p, q| p.wrapping_sub(q)),
        Bin::Mul => zip_with(x, y, t, |p, q| p.wrapping_mul(q)),
        Bin::Div => zip_with(x, y, t, |p, q| if q == 0 { p } else { p.wrapping_div(q) }),
        Bin::Min => zip_with(x, y, t, |p, q| p.min(q)),
        Bin::Max => zip_with(x, y, t, |p, q| p.max(q)),
        Bin::ShiftLeft => zip_with(x, y, t, |p, q| p.wrapping_shl(q as u32 & 31)),
        Bin::ShiftRight => zip_with(x, y, t, |p, q| ((p as u32) >> (q as u32 & 31)) as i32),
        Bin::And => zip_raw(x, y, bool_t, |p, q| ((p != 0) && (q != 0)) as i32),
        Bin::Or => zip_raw(x, y, bool_t, |p, q| ((p != 0) || (q != 0)) as i32),
        Bin::Xor => zip_raw(x, y, bool_t, |p, q| ((p != 0) ^ (q != 0)) as i32),
        Bin::Equal => zip_raw(x, y, bool_t, |p, q| (p == q) as i32),
        Bin::Less => zip_raw(x, y, bool_t, |p, q| (p < q) as i32),
        Bin::Greater => zip_raw(x, y, bool_t, |p, q| (p > q) as i32),
        Bin::LessOrEqual => zip_raw(x, y, bool_t, |p, q| (p <= q) as i32),
        Bin::GreaterOrEqual => zip_raw(x, y, bool_t, |p, q| (p >= q) as i32),
    }
}

fn map_with<F: Fn(i32) -> i32 + Copy>(x: &Tensor, out_t: DType, f: F) -> Tensor {
    let data = match out_t {
        DType::I32 | DType::I64 => x.data.iter().map(|v| f(*v)).collect(),
        _ => x.data.iter().map(|v| out_t.wrap(f(*v))).collect(),
    };
    Tensor { dtype: out_t, shape: x.shape.clone(), data }
}

fn unary(u: Un, x: &Tensor) -> Tensor {
    let t = x.dtype;
    match u {
        Un::Identity => x.clone(),
        Un::Neg => map_with(x, t, i32::wrapping_neg),
        Un::Abs => map_with(x, t, i32::wrapping_abs),
        Un::Sign => map_with(x, t, i32::signum),
        Un::Relu => map_with(x, t, |v| v.max(0)),
        Un::Not => map_with(x, t, |v| (v == 0) as i32),
    }
}

/// `[.., M, K] × [K, N]` (or matching batch dims) with a 32-bit accumulator; `za`/`zb` are zero points
/// subtracted first (0 for plain MatMul). Row-major, i-k-j order so the inner loop vectorises.
fn matmul(a: &Tensor, b: &Tensor, za: &[i32], zb: &[i32], out_t: DType) -> Result<Tensor, String> {
    let (mut ash, mut bsh) = (a.shape.clone(), b.shape.clone());
    let (a1, b1) = (ash.len() == 1, bsh.len() == 1);
    if a1 {
        ash.insert(0, 1);
    }
    if b1 {
        bsh.push(1);
    }
    if ash.len() < 2 || bsh.len() < 2 {
        return Err("MatMul: scalars are not matrices".into());
    }
    let (m, k) = (ash[ash.len() - 2], ash[ash.len() - 1]);
    let (k2, n) = (bsh[bsh.len() - 2], bsh[bsh.len() - 1]);
    if k != k2 {
        return Err(format!("MatMul: {:?} × {:?}: inner sizes {k} and {k2} differ", a.shape, b.shape));
    }
    let (abatch, bbatch) = (&ash[..ash.len() - 2], &bsh[..bsh.len() - 2]);
    let batch = broadcast(abatch, bbatch)?;
    let nb: usize = batch.iter().product();
    let (ia, ib) = (broadcast_index(abatch, &batch), broadcast_index(bbatch, &batch));
    let zpa = |row: usize| if za.len() > 1 { za[row] } else { za.first().copied().unwrap_or(0) };
    let zpb = |col: usize| if zb.len() > 1 { zb[col] } else { zb.first().copied().unwrap_or(0) };
    let shift_b = zb.iter().any(|z| *z != 0);
    let mut out = vec![0i32; nb * m * n];
    let mut brow = vec![0i32; n];
    for (bi, (pa, pb)) in ia.iter().zip(&ib).enumerate() {
        let (oa, ob, oo) = (pa * m * k, pb * k * n, bi * m * n);
        for i in 0..m {
            let acc = &mut out[oo + i * n..oo + (i + 1) * n];
            let za_i = zpa(i);
            for p in 0..k {
                let av = a.data[oa + i * k + p].wrapping_sub(za_i);
                if av == 0 {
                    continue;
                }
                let src = &b.data[ob + p * n..ob + (p + 1) * n];
                if !shift_b {
                    for (o, bv) in acc.iter_mut().zip(src) {
                        *o = o.wrapping_add(av.wrapping_mul(*bv));
                    }
                } else {
                    for (j, (r, bv)) in brow.iter_mut().zip(src).enumerate() {
                        *r = bv.wrapping_sub(zpb(j));
                    }
                    for (o, bv) in acc.iter_mut().zip(&brow) {
                        *o = o.wrapping_add(av.wrapping_mul(*bv));
                    }
                }
            }
        }
    }
    let mut shape = batch;
    if !a1 {
        shape.push(m);
    }
    if !b1 {
        shape.push(n);
    }
    Ok(Tensor { dtype: out_t, shape, data: out.into_iter().map(|v| out_t.wrap(v)).collect() })
}

fn transpose(x: &Tensor, perm: &[usize]) -> Result<Tensor, String> {
    let r = x.shape.len();
    let mut seen = vec![false; r];
    if perm.len() != r || perm.iter().any(|p| *p >= r || std::mem::replace(&mut seen[*p], true)) {
        return Err(format!("Transpose: perm {perm:?} does not fit rank {r}"));
    }
    let shape: Vec<usize> = perm.iter().map(|p| x.shape[*p]).collect();
    let st = strides(&x.shape);
    let pst: Vec<usize> = perm.iter().map(|p| st[*p]).collect();
    let n = x.len();
    let mut data = Vec::with_capacity(n);
    let mut idx = vec![0usize; r];
    let mut at = 0usize;
    for _ in 0..n {
        data.push(x.data[at]);
        for d in (0..r).rev() {
            idx[d] += 1;
            at += pst[d];
            if idx[d] < shape[d] {
                break;
            }
            at -= pst[d] * idx[d];
            idx[d] = 0;
        }
    }
    Ok(Tensor { dtype: x.dtype, shape, data })
}

fn axes_of(op_axes: &Option<Vec<i64>>, ins: &[Option<&Tensor>], at: usize) -> Option<Vec<i64>> {
    op_axes.clone().or_else(|| ins.get(at).copied().flatten().map(|t| t.data.iter().map(|v| *v as i64).collect()))
}

/// Runs one op.
pub fn eval(op: &Op, ins: &[Option<&Tensor>]) -> Result<Tensor, String> {
    match op {
        Op::Bin(b) => binary(*b, need(ins, 0, "binary op")?, need(ins, 1, "binary op")?),
        Op::Fold(b) => {
            let mut acc = need(ins, 0, "Min/Max/Sum")?.clone();
            for t in ins[1..].iter().flatten() {
                acc = binary(*b, &acc, t)?;
            }
            Ok(acc)
        }
        Op::Un(u) => Ok(unary(*u, need(ins, 0, "unary op")?)),
        Op::Clip => {
            let x = need(ins, 0, "Clip")?;
            let lo = ins.get(1).copied().flatten().map_or(i32::MIN, |t| t.data[0]);
            let hi = ins.get(2).copied().flatten().map_or(i32::MAX, |t| t.data[0]);
            Ok(map_with(x, x.dtype, |v| v.max(lo).min(hi)))
        }
        Op::Cast(t) => {
            let x = need(ins, 0, "Cast")?;
            Ok(map_with(x, *t, |v| v))
        }
        Op::Where => {
            let (c, a, b) = (need(ins, 0, "Where")?, need(ins, 1, "Where")?, need(ins, 2, "Where")?);
            same(a, b, "Where")?;
            let shape = broadcast(&broadcast(&c.shape, &a.shape)?, &b.shape)?;
            let (ic, ia, ib) = (broadcast_index(&c.shape, &shape), broadcast_index(&a.shape, &shape), broadcast_index(&b.shape, &shape));
            let data = (0..ic.len()).map(|i| if c.data[ic[i]] != 0 { a.data[ia[i]] } else { b.data[ib[i]] }).collect();
            Ok(Tensor { dtype: a.dtype, shape, data })
        }
        Op::MatMul => {
            let (a, b) = (need(ins, 0, "MatMul")?, need(ins, 1, "MatMul")?);
            same(a, b, "MatMul")?;
            matmul(a, b, &[], &[], a.dtype)
        }
        Op::MatMulInteger => {
            let (a, b) = (need(ins, 0, "MatMulInteger")?, need(ins, 1, "MatMulInteger")?);
            let za = ins.get(2).copied().flatten().map_or(vec![], |t| t.data.clone());
            let zb = ins.get(3).copied().flatten().map_or(vec![], |t| t.data.clone());
            matmul(a, b, &za, &zb, DType::I32)
        }
        Op::Gemm { trans_a, trans_b } => {
            let (mut a, mut b) = (need(ins, 0, "Gemm")?.clone(), need(ins, 1, "Gemm")?.clone());
            same(&a, &b, "Gemm")?;
            if *trans_a {
                a = transpose(&a, &[1, 0])?;
            }
            if *trans_b {
                b = transpose(&b, &[1, 0])?;
            }
            let y = matmul(&a, &b, &[], &[], a.dtype)?;
            match ins.get(2).copied().flatten() {
                Some(c) => binary(Bin::Add, &y, c),
                None => Ok(y),
            }
        }
        Op::Arg { max, axis: ax, keepdims, last } => {
            let x = need(ins, 0, "ArgMax/ArgMin")?;
            let a = axis(*ax, x.shape.len())?;
            let (outer, len, inner) = (x.shape[..a].iter().product::<usize>(), x.shape[a], x.shape[a + 1..].iter().product::<usize>());
            let mut data = Vec::with_capacity(outer * inner);
            for o in 0..outer {
                for i in 0..inner {
                    let mut best = 0usize;
                    for j in 1..len {
                        let (v, bv) = (x.data[(o * len + j) * inner + i], x.data[(o * len + best) * inner + i]);
                        let better = if *max { v > bv || (*last && v == bv) } else { v < bv || (*last && v == bv) };
                        if better {
                            best = j;
                        }
                    }
                    data.push(best as i32);
                }
            }
            let mut shape = x.shape.clone();
            if *keepdims {
                shape[a] = 1;
            } else {
                shape.remove(a);
            }
            Ok(Tensor { dtype: DType::I64, shape, data })
        }
        Op::Reduce { red, axes, keepdims, noop_empty } => {
            let x = need(ins, 0, "Reduce")?;
            let r = x.shape.len();
            let axes = axes_of(axes, ins, 1).unwrap_or_default();
            if axes.is_empty() && *noop_empty {
                return Ok(x.clone());
            }
            let set: Vec<usize> =
                if axes.is_empty() { (0..r).collect() } else { axes.iter().map(|a| axis(*a, r)).collect::<Result<_, _>>()? };
            let kept: Vec<usize> = (0..r).map(|d| if set.contains(&d) { 1 } else { x.shape[d] }).collect();
            let n: usize = kept.iter().product();
            let (init, f): (i32, fn(i32, i32) -> i32) = match red {
                Red::Sum => (0, i32::wrapping_add),
                Red::Max => (i32::MIN, i32::max),
                Red::Min => (i32::MAX, i32::min),
            };
            let mut data = vec![init; n];
            for (src, dst) in broadcast_index(&kept, &x.shape).into_iter().enumerate() {
                data[dst] = f(data[dst], x.data[src]);
            }
            let data = data.into_iter().map(|v| x.dtype.wrap(v)).collect();
            let shape = if *keepdims { kept } else { (0..r).filter(|d| !set.contains(d)).map(|d| x.shape[d]).collect() };
            Ok(Tensor { dtype: x.dtype, shape, data })
        }
        Op::Reshape { allow_zero } => {
            let (x, s) = (need(ins, 0, "Reshape")?, need(ins, 1, "Reshape")?);
            let mut shape: Vec<i64> = s.data.iter().map(|v| *v as i64).collect();
            for (i, d) in shape.iter_mut().enumerate() {
                if *d == 0 && !allow_zero {
                    *d = *x.shape.get(i).ok_or("Reshape: 0 copies a dimension the input does not have")? as i64;
                }
            }
            let known: i64 = shape.iter().filter(|d| **d >= 0).product();
            let holes = shape.iter().filter(|d| **d == -1).count();
            if holes > 1 || shape.iter().any(|d| *d < -1) {
                return Err(format!("Reshape: bad target {shape:?}"));
            }
            let out: Vec<usize> = shape.iter().map(|d| if *d == -1 { x.len() / known.max(1) as usize } else { *d as usize }).collect();
            if out.iter().product::<usize>() != x.len() {
                return Err(format!("Reshape: {:?} cannot become {shape:?}", x.shape));
            }
            Ok(Tensor { dtype: x.dtype, shape: out, data: x.data.clone() })
        }
        Op::Flatten { axis: ax } => {
            let x = need(ins, 0, "Flatten")?;
            let a = if *ax == x.shape.len() as i64 { x.shape.len() } else { axis(*ax, x.shape.len())? };
            let shape = vec![x.shape[..a].iter().product(), x.shape[a..].iter().product()];
            Ok(Tensor { dtype: x.dtype, shape, data: x.data.clone() })
        }
        Op::Squeeze { axes } => {
            let x = need(ins, 0, "Squeeze")?;
            let r = x.shape.len();
            let shape = match axes_of(axes, ins, 1) {
                Some(a) if !a.is_empty() => {
                    let set: Vec<usize> = a.iter().map(|v| axis(*v, r)).collect::<Result<_, _>>()?;
                    if set.iter().any(|d| x.shape[*d] != 1) {
                        return Err(format!("Squeeze: axes {a:?} of {:?} are not all 1", x.shape));
                    }
                    (0..r).filter(|d| !set.contains(d)).map(|d| x.shape[d]).collect()
                }
                _ => x.shape.iter().copied().filter(|d| *d != 1).collect(),
            };
            Ok(Tensor { dtype: x.dtype, shape, data: x.data.clone() })
        }
        Op::Unsqueeze { axes } => {
            let x = need(ins, 0, "Unsqueeze")?;
            let a = axes_of(axes, ins, 1).ok_or("Unsqueeze needs axes")?;
            let r = x.shape.len() + a.len();
            let mut set: Vec<usize> = a.iter().map(|v| axis(*v, r)).collect::<Result<_, _>>()?;
            set.sort_unstable();
            let mut shape = x.shape.clone();
            for d in set {
                shape.insert(d, 1);
            }
            Ok(Tensor { dtype: x.dtype, shape, data: x.data.clone() })
        }
        Op::Concat { axis: ax } => {
            let parts: Vec<&Tensor> = ins.iter().flatten().copied().collect();
            let first = parts.first().ok_or("Concat: no inputs")?;
            let a = axis(*ax, first.shape.len())?;
            let outer: usize = first.shape[..a].iter().product();
            let mut shape = first.shape.clone();
            shape[a] = 0;
            for p in &parts {
                same(first, p, "Concat")?;
                if p.shape.len() != first.shape.len() || p.shape.iter().zip(&first.shape).enumerate().any(|(d, (x, y))| d != a && x != y) {
                    return Err(format!("Concat: {:?} and {:?} differ off axis {a}", first.shape, p.shape));
                }
                shape[a] += p.shape[a];
            }
            let mut data = Vec::with_capacity(shape.iter().product());
            for o in 0..outer {
                for p in &parts {
                    let chunk = p.shape[a..].iter().product::<usize>();
                    data.extend_from_slice(&p.data[o * chunk..(o + 1) * chunk]);
                }
            }
            Ok(Tensor { dtype: first.dtype, shape, data })
        }
        Op::Gather { axis: ax } => {
            let (x, idx) = (need(ins, 0, "Gather")?, need(ins, 1, "Gather")?);
            let a = axis(*ax, x.shape.len())?;
            let (outer, len, inner) = (x.shape[..a].iter().product::<usize>(), x.shape[a], x.shape[a + 1..].iter().product::<usize>());
            let mut data = Vec::with_capacity(outer * idx.len() * inner);
            for o in 0..outer {
                for &i in &idx.data {
                    let j = if i < 0 { i + len as i32 } else { i };
                    if j < 0 || j as usize >= len {
                        return Err(format!("Gather: index {i} out of range 0..{len}"));
                    }
                    let at = (o * len + j as usize) * inner;
                    data.extend_from_slice(&x.data[at..at + inner]);
                }
            }
            let mut shape = x.shape[..a].to_vec();
            shape.extend_from_slice(&idx.shape);
            shape.extend_from_slice(&x.shape[a + 1..]);
            Ok(Tensor { dtype: x.dtype, shape, data })
        }
        Op::Transpose { perm } => {
            let x = need(ins, 0, "Transpose")?;
            let p = perm.clone().unwrap_or_else(|| (0..x.shape.len()).rev().collect());
            transpose(x, &p)
        }
    }
}
