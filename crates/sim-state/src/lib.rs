//! State charts: nested states, layers, reused machines,
//! history, interrupt/back, pick. Knows neither the game nor Rhai: guards (`G`) and
//! actions (`A`) are handles from outside; the `Oracle` evaluates them.
//!
//! Memory is a single string (`encode`/`decode`): active leaves, `#` remembered ones,
//! `^` saved interrupts. A flat machine's state is just the leaf's name.

use std::collections::BTreeMap;

pub type NodeId = usize;

/// Characters not allowed in state names (separators of the memory string).
pub const RESERVED: &[char] = &['.', '|', '#', '^', '=', ','];

/// Maximum nesting depth of interrupts.
pub const MAX_STACK: usize = 16;

/// Step count of an unreachable state.
pub const FAR: i64 = 9_999;

// ---------------------------------------------------------------- definition

#[derive(Clone, Debug)]
pub struct Spec<G, A> {
    pub initial: Option<String>,
    pub states: Vec<(String, Spec<G, A>)>,
    pub layers: Vec<(String, Spec<G, A>)>,
    pub uses: Option<String>,
    pub remember: bool,
    pub pick: Option<PickSpec<G>>,
    pub recheck: bool,
    pub enter: Vec<A>,
    pub exit: Vec<A>,
    pub transitions: Vec<TransitionSpec<G, A>>,
}

impl<G, A> Default for Spec<G, A> {
    fn default() -> Self {
        Spec {
            initial: None,
            states: Vec::new(),
            layers: Vec::new(),
            uses: None,
            remember: false,
            pick: None,
            recheck: false,
            enter: Vec::new(),
            exit: Vec::new(),
            transitions: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    /// The first guard that holds.
    First,
    /// Highest score; on a tie, the one written first.
    Best,
}

#[derive(Clone, Debug)]
pub struct PickSpec<G> {
    pub kind: PickKind,
    pub options: Vec<(String, G)>,
}

#[derive(Clone, Debug)]
pub struct TransitionSpec<G, A> {
    /// Child path (`"Dev.Polish"`) or `"*"` (any child).
    pub from: String,
    pub to: Option<String>,
    pub back: bool,
    pub interrupt: bool,
    pub when: G,
    pub then: Vec<A>,
}

// ---------------------------------------------------------------- compiled chart

#[derive(Clone, Debug)]
pub enum Shape<G> {
    Leaf,
    Or { children: Vec<NodeId>, initial: Option<NodeId>, pick: Option<Pick<G>>, recheck: bool },
    And { layers: Vec<NodeId> },
}

#[derive(Clone, Debug)]
pub struct Pick<G> {
    pub kind: PickKind,
    /// (child, guard/score, salt)
    pub options: Vec<(NodeId, G, u64)>,
}

#[derive(Clone, Debug)]
pub enum From {
    Any,
    Node(NodeId),
}

#[derive(Clone, Debug)]
pub enum To {
    Node(NodeId),
    Back,
}

#[derive(Clone, Debug)]
pub struct Transition<G, A> {
    pub from: From,
    pub to: To,
    pub interrupt: bool,
    pub when: G,
    pub then: Vec<A>,
    pub salt: u64,
}

#[derive(Clone, Debug)]
pub struct Node<G, A> {
    pub name: String,
    pub parent: Option<NodeId>,
    /// Path from the root (`Life.Awake`); the root's path is empty.
    pub path: String,
    pub depth: usize,
    /// Id where the subtree ends (pre-order numbering: subtree = `id..end`).
    pub end: NodeId,
    pub shape: Shape<G>,
    pub remember: bool,
    pub enter: Vec<A>,
    pub exit: Vec<A>,
    pub transitions: Vec<Transition<G, A>>,
    /// Which machine and which of its states this node comes from: (machine, path in machine).
    /// A node mounted via `use` carries both its own place and the mounted machine's root.
    pub origins: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct Chart<G, A> {
    pub nodes: Vec<Node<G, A>>,
    index: BTreeMap<String, NodeId>,
    /// `steps[a][b]`: minimum number of transitions from a to b.
    steps: Vec<Vec<i64>>,
}

/// The side that evaluates guards and scores (Rhai in sim-rules).
pub trait Oracle<G> {
    fn test(&mut self, g: &G, salt: u64) -> Result<bool, String>;
    fn score(&mut self, g: &G, salt: u64) -> Result<i64, String>;
}

/// Without evaluation: `pick` falls back to the default (used at birth).
pub struct NoOracle;

impl<G> Oracle<G> for NoOracle {
    fn test(&mut self, _: &G, _: u64) -> Result<bool, String> {
        Ok(false)
    }
    fn score(&mut self, _: &G, _: u64) -> Result<i64, String> {
        Ok(0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    /// Active leaves, ascending id.
    pub leaves: Vec<NodeId>,
    /// Remembering state → its most recently exited child.
    pub history: BTreeMap<NodeId, NodeId>,
    /// Interrupts: (scope, the leaves in that scope at the time). The last one is on top.
    pub stack: Vec<(NodeId, Vec<NodeId>)>,
}

/// The result of a step.
#[derive(Clone, Debug)]
pub struct Outcome<A> {
    pub mem: Memory,
    /// In order: exit (inside out), the transition's `then`, enter (outside in).
    pub actions: Vec<A>,
    /// Salt of the transition/pick that started this change.
    pub salt: Option<u64>,
    pub changed: bool,
}

// ---------------------------------------------------------------- building

struct Builder<'m, G, A> {
    machines: &'m BTreeMap<String, Spec<G, A>>,
    nodes: Vec<Node<G, A>>,
    errors: Vec<String>,
    using: Vec<String>,
}

impl<G: Clone, A: Clone> Builder<'_, G, A> {
    fn add(&mut self, name: &str, parent: Option<NodeId>, spec: &Spec<G, A>, origin: (String, String)) -> NodeId {
        let id = self.nodes.len();
        let (path, depth) = match parent {
            None => (String::new(), 0),
            Some(p) if self.nodes[p].path.is_empty() => (name.to_string(), 1),
            Some(p) => (format!("{}.{name}", self.nodes[p].path), self.nodes[p].depth + 1),
        };
        if parent.is_some() && (name.is_empty() || name == "*" || name.contains(RESERVED)) {
            self.errors.push(format!("state '{path}': names may not be empty, '*' or contain . | # ^ = ,"));
        }
        self.nodes.push(Node {
            name: name.to_string(),
            parent,
            path: path.clone(),
            depth,
            end: id + 1,
            shape: Shape::Leaf,
            remember: spec.remember,
            enter: spec.enter.clone(),
            exit: spec.exit.clone(),
            transitions: Vec::new(),
            origins: vec![origin.clone()],
        });

        // `use`: the body comes from another machine.
        let mut body_origin = origin;
        let mut pushed = false;
        let body: &Spec<G, A> = match &spec.uses {
            None => spec,
            Some(m) => {
                if !spec.states.is_empty()
                    || !spec.layers.is_empty()
                    || spec.initial.is_some()
                    || spec.pick.is_some()
                    || !spec.transitions.is_empty()
                    || spec.recheck
                {
                    self.errors.push(format!(
                        "state '{path}': `use: \"{m}\"` brings its own states; do not also declare states, layers, initial, pick, recheck or transitions"
                    ));
                }
                if self.using.contains(m) {
                    let chain = [self.using.as_slice(), std::slice::from_ref(m)].concat().join(" -> ");
                    self.errors.push(format!("state '{path}': machine loop {chain} (use `interrupt` for recursion)"));
                    return id;
                }
                let Some(ms) = self.machines.get(m) else {
                    self.errors.push(format!("state '{path}': unknown machine '{m}'"));
                    return id;
                };
                let n = &mut self.nodes[id];
                n.enter.extend(ms.enter.iter().cloned());
                n.exit = ms.exit.iter().cloned().chain(n.exit.drain(..)).collect();
                n.remember |= ms.remember;
                n.origins.push((m.clone(), String::new()));
                body_origin = (m.clone(), String::new());
                self.using.push(m.clone());
                pushed = true;
                ms
            }
        };

        if !body.states.is_empty() && !body.layers.is_empty() {
            self.errors.push(format!("state '{path}': has both states and layers; put the states inside a layer"));
        }
        let child_origin =
            |o: &(String, String), c: &str| (o.0.clone(), if o.1.is_empty() { c.to_string() } else { format!("{}.{c}", o.1) });
        let mut children = Vec::new();
        for (c, cs) in &body.states {
            children.push(self.add(c, Some(id), cs, child_origin(&body_origin, c)));
        }
        let mut layers = Vec::new();
        for (c, cs) in &body.layers {
            layers.push(self.add(c, Some(id), cs, child_origin(&body_origin, c)));
        }
        if pushed {
            self.using.pop();
        }
        self.nodes[id].end = self.nodes.len();

        let child = |b: &Self, rel: &str| b.rel(id, rel);
        let shape = if !children.is_empty() {
            let initial = body.initial.as_ref().and_then(|i| {
                let c = children.iter().copied().find(|&c| self.nodes[c].name == *i);
                if c.is_none() {
                    self.errors.push(format!("state '{path}': initial '{i}' is not one of its states"));
                }
                c
            });
            let pick = body.pick.as_ref().map(|p| Pick {
                kind: p.kind,
                options: p
                    .options
                    .iter()
                    .filter_map(|(o, g)| {
                        let c = children.iter().copied().find(|&c| self.nodes[c].name == *o);
                        if c.is_none() {
                            self.errors.push(format!("state '{path}': pick option '{o}' is not one of its states"));
                        }
                        c.map(|c| (c, g.clone(), 0))
                    })
                    .collect(),
            });
            if initial.is_none() && pick.is_none() && body.initial.is_none() {
                self.errors.push(format!("state '{path}': has states but no `initial` or `pick`"));
            }
            Shape::Or { children, initial, pick, recheck: body.recheck }
        } else if !layers.is_empty() {
            Shape::And { layers }
        } else {
            if body.initial.is_some() || body.pick.is_some() {
                self.errors.push(format!("state '{path}': initial/pick without states"));
            }
            Shape::Leaf
        };
        let is_or = matches!(shape, Shape::Or { .. });
        self.nodes[id].shape = shape;
        if self.nodes[id].remember && !is_or {
            self.errors.push(format!("state '{path}': `remember` needs states inside"));
        }

        for t in &body.transitions {
            let what = format!("state '{path}': transition {} -> {}", t.from, t.to.as_deref().unwrap_or("back"));
            if !is_or {
                self.errors.push(format!("{what}: transitions belong to a state with `states`"));
                continue;
            }
            let from = if t.from == "*" { Some(From::Any) } else { child(self, &t.from).map(From::Node) };
            let to = match (&t.to, t.back) {
                (Some(to), false) => child(self, to).map(To::Node),
                (None, true) => Some(To::Back),
                _ => {
                    self.errors.push(format!("{what}: needs exactly one of `to` and `back: true`"));
                    None
                }
            };
            if t.back && t.interrupt {
                self.errors.push(format!("{what}: `back` and `interrupt` together"));
            }
            match (from, to) {
                (Some(from), Some(to)) => self.nodes[id].transitions.push(Transition {
                    from,
                    to,
                    interrupt: t.interrupt,
                    when: t.when.clone(),
                    then: t.then.clone(),
                    salt: 0,
                }),
                (f, tt) => {
                    if f.is_none() {
                        self.errors.push(format!("{what}: no state '{}' inside '{path}'", t.from));
                    }
                    if tt.is_none()
                        && let Some(to) = &t.to
                    {
                        self.errors.push(format!("{what}: no state '{to}' inside '{path}'"));
                    }
                }
            }
        }
        id
    }

    /// Relative path under `base` (`"Dev.Polish"`).
    fn rel(&self, base: NodeId, rel: &str) -> Option<NodeId> {
        let mut at = base;
        for seg in rel.split('.') {
            at = (at + 1..self.nodes[at].end).find(|&c| self.nodes[c].parent == Some(at) && self.nodes[c].name == seg)?;
        }
        Some(at)
    }
}

impl<G: Clone, A: Clone> Chart<G, A> {
    /// `machines[name]` becomes the root; `use`d machines are mounted in place.
    pub fn build(name: &str, machines: &BTreeMap<String, Spec<G, A>>) -> Result<Self, Vec<String>> {
        let Some(root) = machines.get(name) else { return Err(vec![format!("unknown machine '{name}'")]) };
        let mut b = Builder { machines, nodes: Vec::new(), errors: Vec::new(), using: vec![name.to_string()] };
        b.add("", None, root, (name.to_string(), String::new()));
        if !b.errors.is_empty() {
            return Err(b.errors);
        }
        let mut nodes = b.nodes;
        // Salts: in pre-order, each node's transitions, then its options. In a flat machine transition i → i.
        let mut salt = 0;
        for n in &mut nodes {
            for t in &mut n.transitions {
                t.salt = salt;
                salt += 1;
            }
            if let Shape::Or { pick: Some(p), .. } = &mut n.shape {
                for o in &mut p.options {
                    o.2 = salt;
                    salt += 1;
                }
            }
        }
        let index = nodes.iter().enumerate().skip(1).map(|(i, n)| (n.path.clone(), i)).collect();
        let mut chart = Chart { nodes, index, steps: Vec::new() };
        chart.steps = chart.step_table();
        Ok(chart)
    }
}

impl<G, A: Clone> Chart<G, A> {
    pub fn node(&self, id: NodeId) -> &Node<G, A> {
        &self.nodes[id]
    }

    /// Paths of all states except the root.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.nodes.iter().skip(1).map(|n| n.path.as_str())
    }

    fn inside(&self, a: NodeId, x: NodeId) -> bool {
        a <= x && x < self.nodes[a].end
    }

    fn children(&self, n: NodeId) -> &[NodeId] {
        match &self.nodes[n].shape {
            Shape::Or { children, .. } => children,
            Shape::And { layers } => layers,
            Shape::Leaf => &[],
        }
    }

    /// States whose path ends in `sel`: `"Work"`, `"Awake.Work"`.
    pub fn resolve(&self, sel: &str) -> Vec<NodeId> {
        let segs: Vec<&str> = sel.split('.').collect();
        (1..self.nodes.len())
            .filter(|&i| {
                let p: Vec<&str> = self.nodes[i].path.split('.').collect();
                p.len() >= segs.len() && p[p.len() - segs.len()..] == segs[..]
            })
            .collect()
    }

    /// Nodes originating from (machine, path in machine): where rules written on the machine apply.
    pub fn with_origin(&self, machine: &str, path: &str) -> Vec<NodeId> {
        (0..self.nodes.len()).filter(|&i| self.nodes[i].origins.iter().any(|(m, p)| m == machine && p == path)).collect()
    }

    // ------------------------------------------------------------ memory string

    pub fn encode(&self, m: &Memory) -> String {
        let paths = |ls: &[NodeId]| ls.iter().map(|&l| self.nodes[l].path.as_str()).collect::<Vec<_>>().join("|");
        let mut s = paths(&m.leaves);
        if !m.history.is_empty() {
            s.push('#');
            let hs: Vec<String> = m.history.iter().map(|(&h, &c)| format!("{}={}", self.nodes[h].path, self.nodes[c].name)).collect();
            s.push_str(&hs.join(","));
        }
        for (scope, ls) in &m.stack {
            s.push('^');
            s.push_str(&self.nodes[*scope].path);
            s.push('=');
            s.push_str(&paths(ls));
        }
        s
    }

    pub fn decode(&self, s: &str) -> Result<Memory, String> {
        let find = |p: &str| {
            if p.is_empty() {
                return Ok(0);
            }
            self.index.get(p).copied().ok_or_else(|| format!("unknown state '{p}'"))
        };
        let leaves_of = |t: &str| -> Result<Vec<NodeId>, String> {
            let mut ls = t.split('|').filter(|p| !p.is_empty()).map(find).collect::<Result<Vec<_>, _>>()?;
            ls.sort_unstable();
            Ok(ls)
        };
        let mut parts = s.split('^');
        let head = parts.next().unwrap_or_default();
        let (active, hist) = head.split_once('#').unwrap_or((head, ""));
        let mut m = Memory { leaves: leaves_of(active)?, ..Memory::default() };
        for h in hist.split(',').filter(|h| !h.is_empty()) {
            let (p, c) = h.split_once('=').ok_or_else(|| format!("bad history '{h}'"))?;
            let p = find(p)?;
            let child = self.children(p).iter().copied().find(|&x| self.nodes[x].name == c).ok_or_else(|| format!("bad history '{h}'"))?;
            m.history.insert(p, child);
        }
        for st in parts {
            let (scope, ls) = st.split_once('=').ok_or_else(|| format!("bad interrupt '{st}'"))?;
            m.stack.push((find(scope)?, leaves_of(ls)?));
        }
        Ok(m)
    }

    // ------------------------------------------------------------ queries

    pub fn is_active(&self, m: &Memory, n: NodeId) -> bool {
        m.leaves.iter().any(|&l| self.inside(n, l))
    }

    fn active_child(&self, m: &Memory, n: NodeId) -> Option<NodeId> {
        self.children(n).iter().copied().find(|&c| self.is_active(m, c))
    }

    /// Is one of `ids` active; with `depth`, is the active leaf at most that far below.
    pub fn in_any(&self, m: &Memory, ids: &[NodeId], depth: Option<usize>) -> bool {
        ids.iter()
            .any(|&n| m.leaves.iter().any(|&l| self.inside(n, l) && depth.is_none_or(|d| self.nodes[l].depth - self.nodes[n].depth <= d)))
    }

    /// Depth of the deepest match among the active ones (for glyph selection).
    pub fn deepest(&self, m: &Memory, ids: &[NodeId]) -> Option<usize> {
        ids.iter().filter(|&&n| self.is_active(m, n)).map(|&n| self.nodes[n].depth).max()
    }

    /// Minimum transitions from the active states to one of `ids`. 0 = already there.
    pub fn steps_to(&self, m: &Memory, ids: &[NodeId]) -> i64 {
        let mut best = FAR;
        for (a, row) in self.steps.iter().enumerate() {
            if a == 0 || !self.is_active(m, a) {
                continue;
            }
            for &t in ids {
                best = best.min(row[t]);
            }
        }
        best
    }

    /// Everything that may be entered by default on entering a node (picks optimistic: all).
    fn entry_closure(&self, n: NodeId, out: &mut Vec<NodeId>) {
        out.push(n);
        match &self.nodes[n].shape {
            Shape::Leaf => {}
            Shape::And { layers } => layers.iter().for_each(|&l| self.entry_closure(l, out)),
            Shape::Or { children, initial, pick, .. } => {
                let mut next: Vec<NodeId> = Vec::new();
                if self.nodes[n].remember {
                    next.extend(children);
                }
                next.extend(initial);
                if let Some(p) = pick {
                    next.extend(p.options.iter().map(|o| o.0));
                }
                if next.is_empty() {
                    next.extend(children.first());
                }
                next.sort_unstable();
                next.dedup();
                next.into_iter().for_each(|c| self.entry_closure(c, out));
            }
        }
    }

    fn step_table(&self) -> Vec<Vec<i64>> {
        let n = self.nodes.len();
        // Edge: node → sets reached by one transition (target + its ancestors + default entry).
        let mut edges: Vec<Vec<Vec<NodeId>>> = vec![Vec::new(); n];
        for (l, node) in self.nodes.iter().enumerate() {
            for t in &node.transitions {
                let To::Node(target) = t.to else { continue };
                let mut reach = Vec::new();
                self.entry_closure(target, &mut reach);
                let mut up = self.nodes[target].parent;
                while let Some(p) = up {
                    reach.push(p);
                    up = self.nodes[p].parent;
                }
                let sources: Vec<NodeId> = match t.from {
                    From::Node(f) => vec![f],
                    From::Any => self.children(l).to_vec(),
                };
                for s in sources {
                    edges[s].push(reach.clone());
                }
            }
        }
        (0..n)
            .map(|start| {
                let mut dist = vec![FAR; n];
                dist[start] = 0;
                let mut frontier = vec![start];
                let mut d = 0;
                while !frontier.is_empty() {
                    d += 1;
                    let mut next = Vec::new();
                    for &a in &frontier {
                        for reach in &edges[a] {
                            for &r in reach {
                                if dist[r] > d {
                                    dist[r] = d;
                                    next.push(r);
                                }
                            }
                        }
                    }
                    frontier = next;
                }
                dist
            })
            .collect()
    }

    // ------------------------------------------------------------ changes

    /// Birth: the `initial` (or first option) chain; `enter` does not run.
    pub fn initial(&self) -> Memory {
        let mut none = NoOracle;
        let mut r = Run::new(self, &Memory::default(), &mut none);
        r.enter_to(0, &[]).expect("NoOracle never fails");
        r.mem
    }

    /// One tick: outside in, the first transition that holds at each level; else `recheck`.
    pub fn step(&self, m: &Memory, o: &mut impl Oracle<G>) -> Result<Outcome<A>, String> {
        let mut r = Run::new(self, m, o);
        r.walk(0)?;
        Ok(r.finish())
    }

    /// `Goto`: no change if the target is already active.
    pub fn goto(&self, m: &Memory, target: NodeId, o: &mut impl Oracle<G>) -> Result<Outcome<A>, String> {
        let mut r = Run::new(self, m, o);
        r.go_to(target, false, &[])?;
        Ok(r.finish())
    }

    /// `Interrupt`: save the current place at the target's level, then go.
    pub fn interrupt(&self, m: &Memory, target: NodeId, o: &mut impl Oracle<G>) -> Result<Outcome<A>, String> {
        let scope = self.nodes[target].parent.ok_or("cannot interrupt into the root")?;
        let mut r = Run::new(self, m, o);
        r.push(scope)?;
        r.go_to(target, true, &[])?;
        Ok(r.finish())
    }

    /// `Back`: return to the most recently saved interrupt; else no change.
    pub fn back(&self, m: &Memory, o: &mut impl Oracle<G>) -> Result<Outcome<A>, String> {
        let mut r = Run::new(self, m, o);
        if let Some((scope, leaves)) = r.mem.stack.pop() {
            r.restore(scope, &leaves, &[])?;
        }
        Ok(r.finish())
    }

    /// Is a target valid for `Interrupt` (does its parent have `states`)?
    pub fn interruptible(&self, target: NodeId) -> bool {
        self.nodes[target].parent.is_some_and(|p| matches!(self.nodes[p].shape, Shape::Or { .. }))
    }
}

struct Run<'c, 'o, G, A, O> {
    chart: &'c Chart<G, A>,
    oracle: &'o mut O,
    mem: Memory,
    actions: Vec<A>,
    salt: Option<u64>,
    changed: bool,
}

impl<'c, 'o, G, A: Clone, O: Oracle<G>> Run<'c, 'o, G, A, O> {
    fn new(chart: &'c Chart<G, A>, m: &Memory, oracle: &'o mut O) -> Self {
        Run { chart, oracle, mem: m.clone(), actions: Vec::new(), salt: None, changed: false }
    }

    fn finish(self) -> Outcome<A> {
        Outcome { mem: self.mem, actions: self.actions, salt: self.salt, changed: self.changed }
    }

    fn node(&self, n: NodeId) -> &'c Node<G, A> {
        &self.chart.nodes[n]
    }

    fn walk(&mut self, n: NodeId) -> Result<(), String> {
        let node = self.node(n);
        match &node.shape {
            Shape::Leaf => Ok(()),
            Shape::And { layers } => {
                for &l in layers {
                    self.walk(l)?;
                }
                Ok(())
            }
            Shape::Or { pick, recheck, .. } => {
                for t in &node.transitions {
                    let on = match t.from {
                        From::Any => true,
                        From::Node(f) => self.chart.is_active(&self.mem, f),
                    };
                    if !on || !self.oracle.test(&t.when, t.salt)? {
                        continue;
                    }
                    self.salt.get_or_insert(t.salt);
                    match t.to {
                        To::Node(target) => {
                            if t.interrupt {
                                self.push(n)?;
                            }
                            self.go_to(target, true, &t.then)?;
                        }
                        To::Back => {
                            let saved = self.mem.stack.iter().rposition(|(s, _)| *s == n);
                            let leaves = match saved {
                                Some(i) => {
                                    let (_, ls) = self.mem.stack[i].clone();
                                    self.mem.stack.truncate(i);
                                    ls
                                }
                                None => Vec::new(),
                            };
                            self.restore(n, &leaves, &t.then)?;
                        }
                    }
                    return Ok(());
                }
                let current = self.chart.active_child(&self.mem, n);
                if *recheck && let Some(p) = pick {
                    let chosen = self.choose(n, p, current)?;
                    if Some(chosen) != current {
                        return self.go_to(chosen, false, &[]);
                    }
                }
                match current {
                    Some(c) => self.walk(c),
                    None => Ok(()),
                }
            }
        }
    }

    fn choose(&mut self, n: NodeId, p: &'c Pick<G>, current: Option<NodeId>) -> Result<NodeId, String> {
        let chart = self.chart;
        let fallback = || match &chart.nodes[n].shape {
            Shape::Or { initial: Some(i), .. } => *i,
            _ => p.options.first().map_or_else(|| chart.children(n)[0], |o| o.0),
        };
        match p.kind {
            PickKind::First => {
                for (c, g, salt) in &p.options {
                    if self.oracle.test(g, *salt)? {
                        self.salt.get_or_insert(*salt);
                        return Ok(*c);
                    }
                }
                Ok(fallback())
            }
            PickKind::Best => {
                let mut best: Option<(NodeId, i64, u64)> = None;
                let mut cur_score = None;
                for (c, g, salt) in &p.options {
                    let s = self.oracle.score(g, *salt)?;
                    if Some(*c) == current {
                        cur_score = Some(s);
                    }
                    if best.is_none_or(|(_, b, _)| s > b) {
                        best = Some((*c, s, *salt));
                    }
                }
                let Some((c, s, salt)) = best else { return Ok(fallback()) };
                if let (Some(cur), Some(cs)) = (current, cur_score)
                    && s <= cs
                {
                    return Ok(cur); // on a tie, stay put
                }
                self.salt.get_or_insert(salt);
                Ok(c)
            }
        }
    }

    fn default_child(&mut self, n: NodeId) -> Result<NodeId, String> {
        let node = self.node(n);
        let Shape::Or { children, initial, pick, .. } = &node.shape else { unreachable!("only Or has children") };
        if node.remember
            && let Some(&h) = self.mem.history.get(&n)
        {
            return Ok(h);
        }
        if let Some(p) = pick {
            return self.choose(n, p, None);
        }
        Ok(initial.unwrap_or(children[0]))
    }

    fn exit(&mut self, n: NodeId) {
        match &self.node(n).shape {
            Shape::Leaf => self.mem.leaves.retain(|&l| l != n),
            Shape::Or { .. } => {
                if let Some(c) = self.chart.active_child(&self.mem, n) {
                    self.exit(c);
                }
            }
            Shape::And { layers } => layers.iter().rev().for_each(|&l| self.exit(l)),
        }
        self.actions.extend(self.node(n).exit.iter().cloned());
        if let Some(p) = self.node(n).parent
            && self.node(p).remember
        {
            self.mem.history.insert(p, n);
        }
        let chart = self.chart;
        self.mem.stack.retain(|(s, _)| !chart.inside(n, *s));
        self.changed = true;
    }

    /// Enter `n`; toward the nodes in `goals`, default for the rest.
    fn enter_to(&mut self, n: NodeId, goals: &[NodeId]) -> Result<(), String> {
        self.actions.extend(self.node(n).enter.iter().cloned());
        self.changed = true;
        match &self.node(n).shape {
            Shape::Leaf => {
                let pos = self.mem.leaves.partition_point(|&l| l < n);
                self.mem.leaves.insert(pos, n);
                Ok(())
            }
            Shape::Or { children, .. } => {
                let toward = children.iter().copied().find(|&c| goals.iter().any(|&g| g != n && self.chart.inside(c, g)));
                let c = match toward {
                    Some(c) => c,
                    None => self.default_child(n)?,
                };
                self.enter_to(c, goals)
            }
            Shape::And { layers } => {
                for &l in layers {
                    self.enter_to(l, goals)?;
                }
                Ok(())
            }
        }
    }

    /// Change only what differs. If the target is already active, `reenter` re-enters it.
    fn go_to(&mut self, target: NodeId, reenter: bool, then: &[A]) -> Result<(), String> {
        let mut path = Vec::new();
        let mut at = Some(target);
        while let Some(a) = at {
            path.push(a);
            at = self.node(a).parent;
        }
        path.reverse(); // root, ..., target
        for w in path.windows(2) {
            let (parent, a) = (w[0], w[1]);
            if let Shape::Or { .. } = self.node(parent).shape {
                let c = self.chart.active_child(&self.mem, parent);
                if c != Some(a) {
                    if let Some(c) = c {
                        self.exit(c);
                    }
                    self.actions.extend(then.iter().cloned());
                    return self.enter_to(a, &[target]);
                }
            }
        }
        if reenter {
            self.exit(target);
            self.actions.extend(then.iter().cloned());
            return self.enter_to(target, &[target]);
        }
        self.actions.extend(then.iter().cloned());
        Ok(())
    }

    fn push(&mut self, scope: NodeId) -> Result<(), String> {
        if self.mem.stack.len() >= MAX_STACK {
            return Err(format!("interrupts nested deeper than {MAX_STACK}"));
        }
        let chart = self.chart;
        let saved = self.mem.leaves.iter().copied().filter(|&l| chart.inside(scope, l)).collect();
        self.mem.stack.push((scope, saved));
        self.changed = true;
        Ok(())
    }

    /// Leave `scope`'s child, re-enter the saved leaves (else the default).
    fn restore(&mut self, scope: NodeId, leaves: &[NodeId], then: &[A]) -> Result<(), String> {
        if let Some(c) = self.chart.active_child(&self.mem, scope) {
            self.exit(c);
        }
        self.actions.extend(then.iter().cloned());
        let toward = self.chart.children(scope).iter().copied().find(|&c| leaves.iter().any(|&l| self.chart.inside(c, l)));
        let c = match toward {
            Some(c) => c,
            None => self.default_child(scope)?,
        };
        self.enter_to(c, leaves)
    }
}

// ---------------------------------------------------------------- string queries

/// The active part of a memory string (before `#` and `^`): the state observers see.
pub fn active_part(s: &str) -> &str {
    s.find(['#', '^']).map_or(s, |i| &s[..i])
}

/// End position of `sel` in the leaf's path (deepest), else None.
fn match_end(leaf: &str, sel: &str) -> Option<usize> {
    let segs: Vec<&str> = leaf.split('.').collect();
    let want: Vec<&str> = sel.split('.').collect();
    if want.len() > segs.len() {
        return None;
    }
    (want.len()..=segs.len()).rev().find(|&end| segs[end - want.len()..end] == want[..]).map(|end| segs.len() - end)
}

/// Is the active state string (`Life.Awake.Work|Mood.Tired`) inside `sel`?
pub fn in_label(label: &str, sel: &str) -> bool {
    let label = active_part(label);
    label == sel || label.split('|').any(|leaf| match_end(leaf, sel).is_some())
}

/// How many levels below `sel` the active leaf is; -1 if not inside.
pub fn depth_in_label(label: &str, sel: &str) -> i64 {
    active_part(label).split('|').filter_map(|leaf| match_end(leaf, sel)).min().map_or(-1, |d| d as i64)
}

#[cfg(test)]
mod tests;
