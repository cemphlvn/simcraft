//! sim-kernel: integer ONNX graphs, bit-exact, small enough to give every agent a brain.

use sim_kernel::{Budget, DType, Graph, Model, Tensor, int, mutate, text};

fn t(dtype: DType, shape: &[usize], data: &[i32]) -> Tensor {
    Tensor::new(dtype, shape.to_vec(), data.to_vec()).expect("tensor")
}

/// Deterministic int8 values for weights and observations.
fn noise(n: usize, seed: u64) -> Vec<i32> {
    (0..n).map(|i| (sim_kernel::splitmix64(seed ^ (i as u64 * 7919)) % 255) as i32 - 127).collect()
}

/// An int8 policy, the shape a quantised PyTorch MLP exports to (integer-only):
/// obs [n, IN] → MatMulInteger → + bias → Relu → ÷ 64 → clip to int8 → MatMulInteger → ArgMax = action [n].
const IN: usize = 16;
const HID: usize = 32;
const OUT: usize = 4;

fn policy() -> (Vec<u8>, Vec<i32>, Vec<i32>, Vec<i32>) {
    let (w1, b1, w2) = (noise(IN * HID, 1), noise(HID, 2).iter().map(|v| v * 50).collect::<Vec<_>>(), noise(HID * OUT, 3));
    let mut g = Graph::new("policy");
    let obs = g.input("obs", DType::I8, &[None, Some(IN)]);
    let w1n = g.weight("w1", &t(DType::I8, &[IN, HID], &w1));
    let b1n = g.weight("b1", &t(DType::I32, &[HID], &b1));
    let w2n = g.weight("w2", &t(DType::I8, &[HID, OUT], &w2));
    let scale = g.weight("scale", &Tensor::scalar(DType::I32, 64));
    let lo = g.weight("lo", &Tensor::scalar(DType::I32, -128));
    let hi = g.weight("hi", &Tensor::scalar(DType::I32, 127));
    let h = g.node("MatMulInteger", &[&obs, &w1n], vec![]);
    let h = g.node("Add", &[&h, &b1n], vec![]);
    let h = g.node("Relu", &[&h], vec![]);
    let h = g.node("Div", &[&h, &scale], vec![]);
    let h = g.node("Clip", &[&h, &lo, &hi], vec![]);
    let h = g.node("Cast", &[&h], vec![int("to", 3)]);
    let q = g.node("MatMulInteger", &[&h, &w2n], vec![]);
    let a = g.node("ArgMax", &[&q], vec![int("axis", 1), int("keepdims", 0)]);
    g.output(&a, DType::I64, &[None]);
    (g.encode(), w1, b1, w2)
}

/// The same policy, one agent, written out by hand.
fn reference(obs: &[i32], w1: &[i32], b1: &[i32], w2: &[i32]) -> i32 {
    let h: Vec<i32> = (0..HID)
        .map(|j| {
            let s: i32 = (0..IN).map(|i| obs[i] * w1[i * HID + j]).sum::<i32>() + b1[j];
            (s.max(0) / 64).clamp(-128, 127)
        })
        .collect();
    let q: Vec<i32> = (0..OUT).map(|k| (0..HID).map(|j| h[j] * w2[j * OUT + k]).sum()).collect();
    (0..OUT).fold(0, |best, k| if q[k] > q[best] { k } else { best }) as i32
}

#[test]
fn int8_policy_matches_the_hand_written_reference() {
    let (bytes, w1, b1, w2) = policy();
    let m = Model::load(&bytes, &Budget::default()).expect("loads");
    let n = 200;
    let obs = noise(n * IN, 9);
    let out = m.run(&[t(DType::I8, &[n, IN], &obs)]).expect("runs");
    assert_eq!(out[0].shape, vec![n]);
    for r in 0..n {
        assert_eq!(out[0].data[r], reference(&obs[r * IN..(r + 1) * IN], &w1, &b1, &w2), "row {r}");
    }
    let actions: std::collections::BTreeSet<i32> = out[0].data.iter().copied().collect();
    assert!(actions.len() > 1, "the policy is not constant: {actions:?}");
}

#[test]
fn a_row_does_not_depend_on_its_batch() {
    let (bytes, ..) = policy();
    let m = Model::load(&bytes, &Budget::default()).expect("loads");
    let n = 64;
    let obs = noise(n * IN, 5);
    let all = m.run(&[t(DType::I8, &[n, IN], &obs)]).expect("runs")[0].data.clone();
    for r in 0..n {
        let one = m.run(&[t(DType::I8, &[1, IN], &obs[r * IN..(r + 1) * IN])]).expect("runs");
        assert_eq!(one[0].data[0], all[r]);
    }
}

#[test]
fn a_brain_costs_far_less_than_a_megabyte_per_agent() {
    let (bytes, ..) = policy();
    let m = Model::load(&bytes, &Budget::default()).expect("loads");
    let n = 1000;
    let f = m.footprint(&[t(DType::I8, &[n, IN], &noise(n * IN, 4))]).expect("runs");
    assert!(m.file_bytes < 2 * 1024, "file: {} bytes", m.file_bytes);
    assert_eq!(f.genome_bytes, IN * HID + HID * OUT, "own weights when learning alone: one byte each");
    assert!(f.per_row_bytes < 1024, "per agent: {} bytes", f.per_row_bytes);
    assert!(f.shared_bytes < 16 * 1024, "shared: {} bytes", f.shared_bytes);
}

#[test]
fn genomes_learn_deterministically() {
    let (bytes, ..) = policy();
    let m = Model::load(&bytes, &Budget::default()).expect("loads");
    let n = 32;
    let obs = t(DType::I8, &[n, IN], &noise(n * IN, 6));
    let g = m.genome();
    assert_eq!(m.run_genome(&g, std::slice::from_ref(&obs)).unwrap(), m.run(std::slice::from_ref(&obs)).unwrap(), "own genome = file");
    let child = mutate(&g, 42, 100, 8);
    assert_eq!(child, mutate(&g, 42, 100, 8), "same seed, same child");
    assert_ne!(child, mutate(&g, 43, 100, 8));
    let changed = g.iter().zip(&child).filter(|(a, b)| a != b).count();
    assert!(changed > g.len() / 20 && changed < g.len() / 5, "about 10% of genes change: {changed}/{}", g.len());
    assert!(m.run_genome(&child[1..], &[obs]).is_err(), "wrong genome length is refused");
}

#[test]
fn what_the_kernel_refuses() {
    let load = |g: Graph| Model::load(&g.encode(), &Budget::default()).map(|_| ()).unwrap_err();
    // Floats: different bits on different devices.
    let mut g = Graph::new("f");
    let x = g.input("x", DType::I32, &[None]);
    let mut w = t(DType::I32, &[1], &[0]).to_proto("w");
    w.data_type = 1;
    w.raw_data = 1.0f32.to_le_bytes().to_vec();
    g.tensor(w);
    g.output(&x, DType::I32, &[None]);
    assert!(load(g).contains("float"));
    // Float-scaled quantisation ops, unknown ops.
    let mut g = Graph::new("q");
    let x = g.input("x", DType::I32, &[None]);
    let y = g.node("QuantizeLinear", &[&x], vec![]);
    g.output(&y, DType::I32, &[None]);
    assert!(load(g).contains("integer-only"));
    let mut g = Graph::new("u");
    let x = g.input("x", DType::I32, &[None]);
    let y = g.node("Softmax", &[&x], vec![]);
    g.output(&y, DType::I32, &[None]);
    assert!(load(g).contains("not in the kernel"));
    // Over budget.
    let mut g = Graph::new("big");
    let x = g.input("x", DType::I8, &[None]);
    g.weight("w", &t(DType::I8, &[1 << 20], &vec![1; 1 << 20]));
    g.output(&x, DType::I8, &[None]);
    assert!(load(g).contains("budget"));
}

#[test]
fn integer_semantics_are_wgsl_semantics() {
    let run = |op: &str, a: Tensor, b: Tensor, attrs| {
        let mut g = Graph::new("op");
        let (x, y) = (g.input("a", a.dtype, &[]), g.input("b", b.dtype, &[]));
        let z = g.node(op, &[&x, &y], attrs);
        g.output(&z, a.dtype, &[]);
        Model::load(&g.encode(), &Budget::default()).unwrap().run(&[a, b]).unwrap().remove(0)
    };
    let i32s = |v: &[i32]| t(DType::I32, &[v.len()], v);
    assert_eq!(run("Div", i32s(&[7, -7, 5]), i32s(&[2, 2, 0]), vec![]).data, vec![3, -3, 5], "truncates; x / 0 = x");
    assert_eq!(run("Add", t(DType::I8, &[1], &[120]), t(DType::I8, &[1], &[10]), vec![]).data, vec![-126], "int8 wraps");
    assert_eq!(run("Mul", i32s(&[i32::MAX]), i32s(&[2]), vec![]).data, vec![-2], "int32 wraps");
    assert_eq!(
        run("BitShift", t(DType::U8, &[2], &[1, 200]), t(DType::U8, &[2], &[3, 1]), vec![text("direction", "LEFT")]).data,
        vec![8, 144]
    );
    // Broadcasting: [2, 3] + [3].
    assert_eq!(run("Add", t(DType::I32, &[2, 3], &[0, 1, 2, 3, 4, 5]), i32s(&[10, 20, 30]), vec![]).data, vec![10, 21, 32, 13, 24, 35]);
}

#[test]
fn shape_ops() {
    let mut g = Graph::new("shapes");
    let x = g.input("x", DType::I32, &[Some(2), Some(3)]);
    let shape = g.weight("shape", &t(DType::I64, &[2], &[3, -1]));
    let idx = g.weight("idx", &t(DType::I64, &[2], &[2, 0]));
    let r = g.node("Reshape", &[&x, &shape], vec![]); // [3, 2]
    let tr = g.node("Transpose", &[&r], vec![]); // [2, 3]
    let c = g.node("Concat", &[&x, &tr], vec![int("axis", 0)]); // [4, 3]
    let gt = g.node("Gather", &[&c, &idx], vec![int("axis", 1)]); // [4, 2]
    let s = g.node("ReduceSum", &[&gt], vec![sim_kernel::ints("axes", &[0]), int("keepdims", 0)]); // [2]
    g.output(&s, DType::I32, &[Some(2)]);
    g.output(&tr, DType::I32, &[Some(2), Some(3)]);
    let m = Model::load(&g.encode(), &Budget::default()).expect("loads");
    let out = m.run(&[t(DType::I32, &[2, 3], &[1, 2, 3, 4, 5, 6])]).expect("runs");
    // x = [[1,2,3],[4,5,6]]; reshape [[1,2],[3,4],[5,6]]; transpose [[1,3,5],[2,4,6]].
    assert_eq!(out[1].data, vec![1, 3, 5, 2, 4, 6]);
    // concat rows [1,2,3],[4,5,6],[1,3,5],[2,4,6]; columns 2 and 0 → [3,1],[6,4],[5,1],[6,2]; sums [20, 8].
    assert_eq!(out[0].data, vec![20, 8]);
}
