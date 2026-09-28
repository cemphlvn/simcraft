//! Brains: a kind's integer network (built here, run by `sim-kernel`), and the genomes its entities carry.

use rhai::AST;
use sim_core::splitmix64;
use sim_kernel::{Budget, DType, Graph, Model, Tensor, int};

use crate::game::BrainDef;

/// The bias input every brain gets as its last column (so the genome can learn offsets).
const BIAS: i32 = 64;
/// Hidden activations are divided by this before they are clipped back to int8.
const SCALE: i32 = 128;

pub struct Brain {
    pub def: BrainDef,
    pub model: Model,
    /// Input expressions, in name order.
    pub inputs: Vec<(String, AST)>,
    pub genes: usize,
}

impl Brain {
    pub fn new(def: &BrainDef, inputs: Vec<(String, AST)>) -> Result<Brain, String> {
        if def.outputs.is_empty() {
            return Err("a brain needs at least one output".into());
        }
        if def.hidden.contains(&0) {
            return Err("a hidden layer needs at least one unit".into());
        }
        let bytes = mlp(inputs.len() + 1, &def.hidden, def.outputs.len());
        let model = Model::load(&bytes, &Budget::default())?;
        let genes = model.genome().len();
        Ok(Brain { def: def.clone(), model, inputs, genes })
    }

    /// Random weights in -32..=32, from `seed`.
    pub fn fresh(&self, seed: u64) -> Vec<i8> {
        (0..self.genes).map(|i| (splitmix64(seed ^ (i as u64).wrapping_mul(0x9E37_79B9)) % 65) as i8 - 32).collect()
    }

    /// A newborn's genome: the parent's, mutated (if it is the same kind and heredity is on), else fresh.
    pub fn child(&self, parent: Option<&[i8]>, seed: u64) -> Vec<i8> {
        match parent {
            Some(g) if self.def.inherit && g.len() == self.genes => sim_kernel::mutate(g, seed, self.def.mutation, self.def.step),
            _ => self.fresh(seed),
        }
    }

    /// The chosen output's index for these input values (one agent, its own genome).
    pub fn think(&self, genome: &[i8], values: &[i64]) -> Result<usize, String> {
        let mut row: Vec<i32> = values.iter().map(|v| (*v).clamp(-127, 127) as i32).collect();
        row.push(BIAS);
        let x = Tensor::new(DType::I8, vec![1, row.len()], row)?;
        let out = self.model.run_genome(genome, &[x])?;
        Ok(out[0].data[0] as usize)
    }
}

/// An int8 perceptron: `[1, inputs] → (MatMulInteger → Relu → ÷ SCALE → clip → int8)* → MatMulInteger → ArgMax`.
/// The weights are placeholders (zeros): each entity runs with its own genome.
fn mlp(inputs: usize, hidden: &[usize], outputs: usize) -> Vec<u8> {
    let mut g = Graph::new("brain");
    let mut x = g.input("in", DType::I8, &[Some(1), Some(inputs)]);
    let scale = g.weight("scale", &Tensor::scalar(DType::I32, SCALE));
    let (lo, hi) = (g.weight("lo", &Tensor::scalar(DType::I32, 0)), g.weight("hi", &Tensor::scalar(DType::I32, 127)));
    let mut width = inputs;
    for (l, h) in hidden.iter().enumerate() {
        let w = g.weight(&format!("w{l}"), &Tensor { dtype: DType::I8, shape: vec![width, *h], data: vec![0; width * h] });
        let y = g.node("MatMulInteger", &[&x, &w], vec![]);
        let y = g.node("Relu", &[&y], vec![]);
        let y = g.node("Div", &[&y, &scale], vec![]);
        let y = g.node("Clip", &[&y, &lo, &hi], vec![]);
        x = g.node("Cast", &[&y], vec![int("to", 3)]);
        width = *h;
    }
    let w = g.weight("out", &Tensor { dtype: DType::I8, shape: vec![width, outputs], data: vec![0; width * outputs] });
    let q = g.node("MatMulInteger", &[&x, &w], vec![]);
    let a = g.node("ArgMax", &[&q], vec![int("axis", 1), int("keepdims", 0)]);
    g.output(&a, DType::I64, &[Some(1)]);
    g.encode()
}
