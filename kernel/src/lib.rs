//! sim-kernel: a 32-bit integer tensor machine that runs ONNX graphs.
//!
//! One model = one `.onnx` file: a brain (observations → action scores) or a rule (props → props). Every
//! entity of a kind is one row of a batch, so the weights are shared and each agent costs only its row.
//! Integer only, wrapping at 32 bits like WGSL: the same model gives the same bits on every CPU, in wasm
//! and (later) on the GPU. Knows nothing about games.

pub mod ops;
pub mod proto;
pub mod tensor;

use std::collections::BTreeMap;

use prost::Message;

pub use ops::Op;
use proto::{AttributeProto, GraphProto, ModelProto, NodeProto, OperatorSetIdProto, TypeProto, TypeTensor, ValueInfoProto};
pub use tensor::{DType, Tensor};

/// What a model may cost. A model over budget does not load.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    /// The `.onnx` file, weights included. Shared by every agent that uses it.
    pub model_bytes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Budget { model_bytes: 1 << 20 }
    }
}

/// A graph input or output: name, type, dimensions (`None` = any size, e.g. the batch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    pub name: String,
    pub dtype: DType,
    pub dims: Vec<Option<usize>>,
}

#[derive(Clone, Debug)]
struct Node {
    name: String,
    op: Op,
    ins: Vec<Option<usize>>,
    out: usize,
}

/// A loaded, checked model, ready to run.
#[derive(Clone, Debug)]
pub struct Model {
    pub name: String,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    /// Size of the `.onnx` file.
    pub file_bytes: usize,
    /// Weights and constants by value slot.
    consts: Vec<Option<Tensor>>,
    nodes: Vec<Node>,
    input_slots: Vec<usize>,
    output_slots: Vec<usize>,
    /// Slots of the int8 weights, in file order: what learning changes (see `genome`).
    genes: Vec<usize>,
}

/// Memory one run of a model needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Footprint {
    /// Weights, as held in memory, shared by all rows.
    pub shared_bytes: usize,
    /// Every intermediate value of one row (one agent).
    pub per_row_bytes: usize,
    /// One agent's own weights when it learns on its own (`genome`), in bytes.
    pub genome_bytes: usize,
}

impl Model {
    pub fn load(bytes: &[u8], budget: &Budget) -> Result<Model, String> {
        if bytes.len() > budget.model_bytes {
            return Err(format!("model is {} bytes; the budget is {} bytes", bytes.len(), budget.model_bytes));
        }
        let m = ModelProto::decode(bytes).map_err(|e| format!("not an ONNX model: {e}"))?;
        let g = m.graph.ok_or("the model has no graph")?;
        for o in &m.opset_import {
            if !(o.domain.is_empty() || o.domain == "ai.onnx") {
                return Err(format!("operator set '{}' is not supported (only the standard ONNX set)", o.domain));
            }
        }
        let mut slots: BTreeMap<String, usize> = BTreeMap::new();
        let mut consts: Vec<Option<Tensor>> = Vec::new();
        fn slot_of(slots: &mut BTreeMap<String, usize>, name: &str, consts: &mut Vec<Option<Tensor>>) -> usize {
            *slots.entry(name.to_string()).or_insert_with(|| {
                consts.push(None);
                consts.len() - 1
            })
        }
        let mut genes = Vec::new();
        for t in &g.initializer {
            let s = slot_of(&mut slots, &t.name, &mut consts);
            let v = Tensor::from_proto(t)?;
            if v.dtype == DType::I8 {
                genes.push(s);
            }
            consts[s] = Some(v);
        }
        let port = |v: &ValueInfoProto| -> Result<Port, String> {
            let tt = v.r#type.as_ref().and_then(|t| t.tensor_type.as_ref()).ok_or_else(|| format!("'{}' has no tensor type", v.name))?;
            let dtype = DType::from_onnx(tt.elem_type).map_err(|e| format!("'{}': {e}", v.name))?;
            let dims = tt.shape.as_ref().map_or(vec![], |s| s.dim.iter().map(|d| d.dim_value.map(|x| x as usize)).collect());
            Ok(Port { name: v.name.clone(), dtype, dims })
        };
        let mut inputs = Vec::new();
        let mut input_slots = Vec::new();
        for v in g.input.iter().filter(|v| !g.initializer.iter().any(|t| t.name == v.name)) {
            inputs.push(port(v)?);
            input_slots.push(slot_of(&mut slots, &v.name, &mut consts));
        }
        let mut defined: Vec<bool> = vec![true; consts.len()];
        let mut nodes = Vec::new();
        for (i, n) in g.node.iter().enumerate() {
            let label = if n.name.is_empty() { format!("node {i} ({})", n.op_type) } else { format!("'{}' ({})", n.name, n.op_type) };
            if !(n.domain.is_empty() || n.domain == "ai.onnx") {
                return Err(format!("{label}: domain '{}' is not supported", n.domain));
            }
            let outs: Vec<&String> = n.output.iter().filter(|o| !o.is_empty()).collect();
            let [out] = outs.as_slice() else { return Err(format!("{label}: exactly one output is supported")) };
            if n.op_type == "Constant" {
                let a = n.attribute.first().ok_or_else(|| format!("{label}: no value"))?;
                let v = match a.name.as_str() {
                    "value" => Tensor::from_proto(a.t.as_ref().ok_or_else(|| format!("{label}: empty value"))?)?,
                    "value_int" => Tensor::scalar(DType::I64, narrow(a.i, &label)?),
                    "value_ints" => {
                        Tensor::new(DType::I64, vec![a.ints.len()], a.ints.iter().map(|v| narrow(*v, &label)).collect::<Result<_, _>>()?)?
                    }
                    other => return Err(format!("{label}: '{other}' constants are not supported (int tensors only)")),
                };
                let s = slot_of(&mut slots, out, &mut consts);
                consts[s] = Some(v);
                defined.resize(consts.len(), false);
                defined[s] = true;
                continue;
            }
            let op = ops::parse(&n.op_type, &n.attribute).map_err(|e| format!("{label}: {e}"))?;
            let mut ins = Vec::new();
            for x in &n.input {
                if x.is_empty() {
                    ins.push(None);
                    continue;
                }
                let s = *slots.get(x).ok_or_else(|| format!("{label}: input '{x}' is not defined before it (nodes must be in order)"))?;
                if !defined.get(s).copied().unwrap_or(false) {
                    return Err(format!("{label}: input '{x}' is not defined before it (nodes must be in order)"));
                }
                ins.push(Some(s));
            }
            let s = slot_of(&mut slots, out, &mut consts);
            defined.resize(consts.len(), false);
            defined[s] = true;
            nodes.push(Node { name: label, op, ins, out: s });
        }
        let mut outputs = Vec::new();
        let mut output_slots = Vec::new();
        for v in &g.output {
            let s = *slots.get(&v.name).ok_or_else(|| format!("output '{}' is never computed", v.name))?;
            outputs.push(port(v)?);
            output_slots.push(s);
        }
        Ok(Model { name: g.name, inputs, outputs, file_bytes: bytes.len(), consts, nodes, input_slots, output_slots, genes })
    }

    /// Runs the graph. Inputs in the order of `self.inputs`.
    pub fn run(&self, inputs: &[Tensor]) -> Result<Vec<Tensor>, String> {
        self.exec(inputs, None, &mut |_| {})
    }

    /// Runs with this agent's own int8 weights in place of the file's (same length as `genome()`).
    pub fn run_genome(&self, genome: &[i8], inputs: &[Tensor]) -> Result<Vec<Tensor>, String> {
        self.exec(inputs, Some(genome), &mut |_| {})
    }

    /// The model's int8 weights, flattened in file order: what an agent inherits, mutates and learns.
    pub fn genome(&self) -> Vec<i8> {
        self.genes.iter().flat_map(|s| self.consts[*s].as_ref().expect("weight").data.iter().map(|v| *v as i8)).collect()
    }

    /// What a run with `inputs` costs; rows = the first dimension of the first input.
    pub fn footprint(&self, inputs: &[Tensor]) -> Result<Footprint, String> {
        let rows = inputs.first().and_then(|t| t.shape.first()).copied().unwrap_or(1).max(1);
        let mut activations = 0usize;
        self.exec(inputs, None, &mut |t| activations += t.len() * 4)?;
        let shared_bytes = self.consts.iter().flatten().map(|t| t.len() * 4).sum();
        Ok(Footprint { shared_bytes, per_row_bytes: activations.div_ceil(rows), genome_bytes: self.genome().len() })
    }

    fn exec(&self, inputs: &[Tensor], genome: Option<&[i8]>, seen: &mut dyn FnMut(&Tensor)) -> Result<Vec<Tensor>, String> {
        if inputs.len() != self.inputs.len() {
            return Err(format!("model '{}' takes {} inputs, got {}", self.name, self.inputs.len(), inputs.len()));
        }
        let mut vals: Vec<Option<Tensor>> = vec![None; self.consts.len()];
        for ((p, t), s) in self.inputs.iter().zip(inputs).zip(&self.input_slots) {
            if t.dtype != p.dtype {
                return Err(format!("input '{}' is {:?}, got {:?}", p.name, p.dtype, t.dtype));
            }
            if !p.dims.is_empty() && (p.dims.len() != t.shape.len() || p.dims.iter().zip(&t.shape).any(|(d, n)| d.is_some_and(|d| d != *n)))
            {
                return Err(format!("input '{}' has shape {:?}, got {:?}", p.name, p.dims, t.shape));
            }
            vals[*s] = Some(t.clone());
        }
        if let Some(g) = genome {
            let want: usize = self.genes.iter().map(|s| self.consts[*s].as_ref().map_or(0, Tensor::len)).sum();
            if g.len() != want {
                return Err(format!("genome has {} genes, the model has {want} int8 weights", g.len()));
            }
            let mut at = 0;
            for s in &self.genes {
                let w = self.consts[*s].as_ref().expect("weight");
                vals[*s] = Some(Tensor {
                    dtype: DType::I8,
                    shape: w.shape.clone(),
                    data: g[at..at + w.len()].iter().map(|v| *v as i32).collect(),
                });
                at += w.len();
            }
        }
        for n in &self.nodes {
            let ins: Vec<Option<&Tensor>> = n.ins.iter().map(|s| s.and_then(|s| vals[s].as_ref().or(self.consts[s].as_ref()))).collect();
            let out = ops::eval(&n.op, &ins).map_err(|e| format!("{}: {e}", n.name))?;
            seen(&out);
            vals[n.out] = Some(out);
        }
        self.output_slots
            .iter()
            .zip(&self.outputs)
            .map(|(s, p)| {
                vals[*s].clone().or_else(|| self.consts[*s].clone()).ok_or_else(|| format!("output '{}' was not computed", p.name))
            })
            .collect()
    }
}

fn narrow(v: i64, what: &str) -> Result<i32, String> {
    i32::try_from(v).map_err(|_| format!("{what}: {v} does not fit in 32 bits"))
}

/// splitmix64: the kernel's only source of randomness (seeded, stateless).
pub fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A child genome: each gene changes with probability `per_mille`/1000 by ±1..=`step`, saturating at the int8
/// range. Deterministic in `seed`.
pub fn mutate(genome: &[i8], seed: u64, per_mille: u32, step: i8) -> Vec<i8> {
    let step = step.max(1) as u64;
    genome
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let h = splitmix64(seed ^ (i as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93));
            if (h % 1000) as u32 >= per_mille {
                return *g;
            }
            let d = ((h >> 10) % step + 1) as i16;
            let d = if (h >> 40) & 1 == 0 { d } else { -d };
            (*g as i16 + d).clamp(-128, 127) as i8
        })
        .collect()
}

/// Builds ONNX models in code (tests, and the compiler that turns rules into graphs).
#[derive(Default)]
pub struct Graph {
    g: GraphProto,
    next: usize,
}

impl Graph {
    pub fn new(name: &str) -> Graph {
        Graph { g: GraphProto { name: name.into(), ..Default::default() }, next: 0 }
    }

    fn value_info(name: &str, dtype: DType, dims: &[Option<usize>]) -> ValueInfoProto {
        let dim = dims
            .iter()
            .map(|d| proto::Dimension { dim_value: d.map(|v| v as i64), dim_param: d.is_none().then(|| "n".to_string()) })
            .collect();
        ValueInfoProto {
            name: name.into(),
            r#type: Some(TypeProto {
                tensor_type: Some(TypeTensor { elem_type: dtype.onnx(), shape: Some(proto::TensorShapeProto { dim }) }),
            }),
        }
    }

    pub fn input(&mut self, name: &str, dtype: DType, dims: &[Option<usize>]) -> String {
        self.g.input.push(Self::value_info(name, dtype, dims));
        name.into()
    }

    pub fn weight(&mut self, name: &str, t: &Tensor) -> String {
        self.g.initializer.push(t.to_proto(name));
        name.into()
    }

    /// Adds an initializer exactly as given (e.g. one in a type the kernel refuses).
    pub fn tensor(&mut self, t: proto::TensorProto) {
        self.g.initializer.push(t);
    }

    /// Adds a node; returns its output's name. Attributes: `int`, `ints`, `text`.
    pub fn node(&mut self, op: &str, inputs: &[&str], attrs: Vec<AttributeProto>) -> String {
        self.next += 1;
        let out = format!("{}_{}", op.to_lowercase(), self.next);
        self.g.node.push(NodeProto {
            input: inputs.iter().map(|s| s.to_string()).collect(),
            output: vec![out.clone()],
            name: out.clone(),
            op_type: op.into(),
            attribute: attrs,
            domain: String::new(),
        });
        out
    }

    pub fn output(&mut self, value: &str, dtype: DType, dims: &[Option<usize>]) {
        self.g.output.push(Self::value_info(value, dtype, dims));
    }

    pub fn encode(self) -> Vec<u8> {
        ModelProto {
            ir_version: 9,
            producer_name: "simcraft".into(),
            graph: Some(self.g),
            opset_import: vec![OperatorSetIdProto { domain: String::new(), version: 19 }],
        }
        .encode_to_vec()
    }
}

pub fn int(name: &str, v: i64) -> AttributeProto {
    AttributeProto { name: name.into(), i: v, r#type: 2, ..Default::default() }
}

pub fn ints(name: &str, v: &[i64]) -> AttributeProto {
    AttributeProto { name: name.into(), ints: v.to_vec(), r#type: 7, ..Default::default() }
}

pub fn text(name: &str, v: &str) -> AttributeProto {
    AttributeProto { name: name.into(), s: v.as_bytes().to_vec(), r#type: 3, ..Default::default() }
}
