//! Rules compiled once, run natively (docs/research/physics-engines.md §6.3; closure compilation, Feeley and
//! Lapalme 1987, as a tree over native values): the common subset of rule expressions is parsed at load time into
//! a small tree evaluated straight from the entity, with no Rhai scope, no `Dynamic`, no string-keyed maps for `p`.
//!
//! The subset: integer literals and strings (as function arguments), `me.*`, `it.*`, `arg.*`, `sense.*`, `p.*`,
//! `roll`, `tick`, `tick_rate`, `+ - * / %`, comparisons, `&& || !`, unary minus, parentheses, `if c { a } else
//! { b }`, blocks with `let`, calls to the engine's functions (`ahead`, `under`, `prop_of`, `min`, `rand`...), and
//! for scripts a final array of effect maps (`#{ op: "set" | "add", prop: "..", value: .. }`). Anything else is not
//! compiled: the interpreter runs it, as before.
//!
//! Exactness: evaluation keeps Rhai's order (left to right, short-circuit `&&`/`||`), so functions with a hidden
//! counter (`rand`) see the same calls. Anything unexpected at run time (overflow, division by zero, a missing prop,
//! a sense of another type) is a `Fallback`: the caller rewinds and asks the interpreter, which then gives Rhai's own
//! answer or error. `fast_paths_change_nothing` runs every game both ways and compares hashes.

use std::collections::BTreeMap;

/// A value: rule expressions compute integers and booleans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Val {
    I(i64),
    B(bool),
}

/// Not answered natively: let the interpreter answer (it knows the exact result or error).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fallback;

type R<T> = Result<T, Fallback>;

/// Engine functions a compiled expression can call (resolved at load time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Func {
    Ahead,
    Behind,
    AheadId,
    BehindId,
    Touching,
    Under,
    PropOf,
    NearestProp,
    Around,
    InState,
    Rand,
    Pace,
    Min,
    Max,
    Abs,
    Clamp,
}

impl Func {
    fn of(name: &str) -> Option<Func> {
        Some(match name {
            "ahead" => Func::Ahead,
            "behind" => Func::Behind,
            "ahead_id" => Func::AheadId,
            "behind_id" => Func::BehindId,
            "touching" => Func::Touching,
            "under" => Func::Under,
            "prop_of" => Func::PropOf,
            "nearest_prop" => Func::NearestProp,
            "around" => Func::Around,
            "in_state" => Func::InState,
            "rand" => Func::Rand,
            "pace" => Func::Pace,
            "min" => Func::Min,
            "max" => Func::Max,
            "abs" => Func::Abs,
            "clamp" => Func::Clamp,
            _ => return None,
        })
    }
}

/// A function argument: a value, or a string literal (a kind, a prop, a state).
#[derive(Clone, Debug)]
pub enum Arg {
    V(Val),
    S(String),
}

/// What an expression reads and calls: the entity being evaluated, its target, args, senses, and the engine.
pub trait Env {
    fn me(&self, field: &str) -> Option<i64>;
    fn it(&self, field: &str) -> Option<i64>;
    fn arg(&self, name: &str) -> Option<i64>;
    fn sense(&self, name: &str) -> Option<Val>;
    /// `roll`, `tick`: what this scope has (None if the interpreter would not see it here).
    fn var(&self, name: &str) -> Option<i64>;
    /// An engine function (None: not answerable natively here).
    fn call(&self, f: Func, args: &[Arg]) -> Option<Val>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

/// A compiled expression.
#[derive(Clone, Debug)]
pub enum Expr {
    Lit(Val),
    Str(String),
    Me(String),
    It(String),
    Arg(String),
    Sense(String),
    Var(String),
    Local(usize),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `{ let a = ..; let b = ..; value }`: each `let` into its own slot, then the value.
    Block(Vec<(usize, Expr)>, Box<Expr>),
    Call(Func, Vec<Expr>),
    /// A script's result: effects (op is "set" or "add", the prop, the value).
    Effects(Vec<(bool, String, Expr)>),
}

/// A compiled script: its effects, as (is `add`, prop, value).
pub type Effects = Vec<(bool, String, i64)>;

// ---------------------------------------------------------------- tokens

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(i64),
    Str(String),
    Id(String),
    Sym(&'static str),
}

fn lex(src: &str) -> Option<Vec<Tok>> {
    const SYMS: [&str; 26] = [
        "#{", "&&", "||", "==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%", "!", "(", ")", "{", "}", "[", "]", ",", ";", ".", ":",
        "=",
    ];
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let s = i;
            while i < b.len() && (b[i] as char).is_ascii_digit() {
                i += 1;
            }
            out.push(Tok::Num(src[s..i].parse().ok()?));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let s = i;
            while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Id(src[s..i].to_string()));
        } else if c == '"' {
            let s = i + 1;
            i = s;
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' {
                    return None;
                }
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
            out.push(Tok::Str(src[s..i].to_string()));
            i += 1;
        } else {
            let sym = SYMS.iter().find(|s| src[i..].starts_with(**s))?;
            out.push(Tok::Sym(sym));
            i += sym.len();
        }
    }
    Some(out)
}

// ---------------------------------------------------------------- parser

struct Parser<'a> {
    t: Vec<Tok>,
    i: usize,
    params: &'a BTreeMap<String, i64>,
    tick_rate: i64,
    /// Names of `let` slots in scope (innermost last) and how many slots were ever made.
    locals: Vec<(String, usize)>,
    slots: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn is(&self, s: &str) -> bool {
        matches!(self.peek(), Some(Tok::Sym(x)) if *x == s)
    }
    fn eat(&mut self, s: &str) -> Option<()> {
        self.is(s).then(|| self.i += 1)
    }
    fn id(&mut self) -> Option<String> {
        match self.peek()? {
            Tok::Id(s) => {
                let s = s.clone();
                self.i += 1;
                Some(s)
            }
            _ => None,
        }
    }

    /// Statements then a value, up to (not including) `}` or the end.
    fn body(&mut self) -> Option<Expr> {
        let depth = self.locals.len();
        let mut lets = Vec::new();
        while matches!(self.peek(), Some(Tok::Id(k)) if k == "let") {
            self.i += 1;
            let name = self.id()?;
            self.eat("=")?;
            // The value first (it may hold blocks with lets of their own), then this let's own slot.
            let v = self.expr(0)?;
            self.eat(";")?;
            let slot = self.slots;
            self.slots += 1;
            lets.push((slot, v));
            self.locals.push((name, slot));
        }
        let value = self.expr(0)?;
        self.eat(";");
        self.locals.truncate(depth);
        Some(if lets.is_empty() { value } else { Expr::Block(lets, Box::new(value)) })
    }

    fn block(&mut self) -> Option<Expr> {
        self.eat("{")?;
        let b = self.body()?;
        self.eat("}")?;
        Some(b)
    }

    /// The binary operator at the cursor and its precedence (`||` lowest), if there is one.
    fn binop(&self) -> Option<(Op, u8)> {
        let Some(Tok::Sym(s)) = self.peek() else { return None };
        Some(match *s {
            "||" => (Op::Or, 1),
            "&&" => (Op::And, 2),
            "==" => (Op::Eq, 3),
            "!=" => (Op::Ne, 3),
            "<" => (Op::Lt, 4),
            "<=" => (Op::Le, 4),
            ">" => (Op::Gt, 4),
            ">=" => (Op::Ge, 4),
            "+" => (Op::Add, 5),
            "-" => (Op::Sub, 5),
            "*" => (Op::Mul, 6),
            "/" => (Op::Div, 6),
            "%" => (Op::Rem, 6),
            _ => return None,
        })
    }

    fn expr(&mut self, min: u8) -> Option<Expr> {
        let mut lhs = self.unary()?;
        while let Some((op, prec)) = self.binop() {
            if prec < min {
                break;
            }
            self.i += 1;
            let rhs = self.expr(prec + 1)?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }

    fn unary(&mut self) -> Option<Expr> {
        if self.eat("-").is_some() {
            // A negative literal is a literal (as Rhai folds it).
            if let Some(Tok::Num(n)) = self.peek() {
                let n = *n;
                self.i += 1;
                return Some(Expr::Lit(Val::I(-n)));
            }
            return Some(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat("!").is_some() {
            return Some(Expr::Not(Box::new(self.unary()?)));
        }
        self.atom()
    }

    fn atom(&mut self) -> Option<Expr> {
        match self.peek()?.clone() {
            Tok::Num(n) => {
                self.i += 1;
                Some(Expr::Lit(Val::I(n)))
            }
            Tok::Str(s) => {
                self.i += 1;
                Some(Expr::Str(s))
            }
            Tok::Sym("(") => {
                self.i += 1;
                let e = self.expr(0)?;
                self.eat(")")?;
                Some(e)
            }
            Tok::Sym("{") => self.block(),
            Tok::Sym("[") => self.effects(),
            Tok::Id(name) => {
                self.i += 1;
                match name.as_str() {
                    "true" => Some(Expr::Lit(Val::B(true))),
                    "false" => Some(Expr::Lit(Val::B(false))),
                    "if" => {
                        let c = self.expr(0)?;
                        let a = self.block()?;
                        match self.peek() {
                            Some(Tok::Id(k)) if k == "else" => self.i += 1,
                            _ => return None,
                        }
                        let b = if matches!(self.peek(), Some(Tok::Id(k)) if k == "if") { self.atom()? } else { self.block()? };
                        Some(Expr::If(Box::new(c), Box::new(a), Box::new(b)))
                    }
                    "me" | "it" | "arg" | "sense" | "p" => {
                        self.eat(".")?;
                        let field = self.id()?;
                        Some(match name.as_str() {
                            "me" => Expr::Me(field),
                            "it" => Expr::It(field),
                            "arg" => Expr::Arg(field),
                            "sense" => Expr::Sense(field),
                            _ => Expr::Lit(Val::I(*self.params.get(&field)?)),
                        })
                    }
                    "roll" | "tick" => Some(Expr::Var(name)),
                    "tick_rate" => Some(Expr::Lit(Val::I(self.tick_rate))),
                    _ if self.is("(") => {
                        let f = Func::of(&name)?;
                        self.i += 1;
                        let mut args = Vec::new();
                        while !self.is(")") {
                            args.push(self.expr(0)?);
                            if self.eat(",").is_none() {
                                break;
                            }
                        }
                        self.eat(")")?;
                        Some(Expr::Call(f, args))
                    }
                    _ => {
                        let slot = self.locals.iter().rev().find(|(n, _)| *n == name)?.1;
                        Some(Expr::Local(slot))
                    }
                }
            }
            _ => None,
        }
    }

    /// `[ #{ op: "set", prop: "vy", value: .. }, .. ]`
    fn effects(&mut self) -> Option<Expr> {
        self.eat("[")?;
        let mut out = Vec::new();
        while !self.is("]") {
            self.eat("#{")?;
            let (mut op, mut prop, mut value) = (None, None, None);
            while !self.is("}") {
                let key = self.id()?;
                self.eat(":")?;
                match key.as_str() {
                    "op" | "prop" => {
                        let Some(Tok::Str(s)) = self.peek().cloned() else { return None };
                        self.i += 1;
                        if key == "op" { op = Some(s) } else { prop = Some(s) }
                    }
                    "value" => value = Some(self.expr(0)?),
                    _ => return None,
                }
                if self.eat(",").is_none() {
                    break;
                }
            }
            self.eat("}")?;
            let add = match op?.as_str() {
                "set" => false,
                "add" => true,
                _ => return None,
            };
            out.push((add, prop?, value?));
            if self.eat(",").is_none() {
                break;
            }
        }
        self.eat("]")?;
        Some(Expr::Effects(out))
    }
}

/// Compiles an expression or a script (statements, then a value). None: not in the subset (the interpreter runs it).
pub fn compile(src: &str, params: &BTreeMap<String, i64>, tick_rate: i64) -> Option<Compiled> {
    let t = lex(src)?;
    let mut p = Parser { t, i: 0, params, tick_rate, locals: Vec::new(), slots: 0 };
    let e = p.body()?;
    (p.i == p.t.len() && strings_only_as_args(&e, false)).then_some(Compiled { expr: e, slots: p.slots })
}

/// A string is a function's argument (a kind, a prop, a state), never a value: anywhere else the interpreter runs
/// the expression (its strings, its errors).
fn strings_only_as_args(e: &Expr, arg: bool) -> bool {
    match e {
        Expr::Str(_) => arg,
        Expr::Neg(x) | Expr::Not(x) => strings_only_as_args(x, false),
        Expr::Bin(_, a, b) => strings_only_as_args(a, false) && strings_only_as_args(b, false),
        Expr::If(c, a, b) => [c, a, b].iter().all(|x| strings_only_as_args(x, false)),
        Expr::Block(lets, v) => lets.iter().all(|(_, l)| strings_only_as_args(l, false)) && strings_only_as_args(v, false),
        Expr::Call(_, args) => args.iter().all(|a| strings_only_as_args(a, true)),
        Expr::Effects(list) => list.iter().all(|(_, _, v)| strings_only_as_args(v, false)),
        _ => true,
    }
}

/// A compiled expression and how many `let` slots it needs.
#[derive(Clone, Debug)]
pub struct Compiled {
    expr: Expr,
    slots: usize,
}

impl Compiled {
    pub fn int(&self, env: &dyn Env) -> R<i64> {
        match self.run(env)? {
            Val::I(v) => Ok(v),
            Val::B(_) => Err(Fallback),
        }
    }

    pub fn bool(&self, env: &dyn Env) -> R<bool> {
        match self.run(env)? {
            Val::B(v) => Ok(v),
            Val::I(_) => Err(Fallback),
        }
    }

    /// A script's effects.
    pub fn effects(&self, env: &dyn Env) -> R<Effects> {
        let mut slots = vec![Val::I(0); self.slots];
        effects_of(&self.expr, env, &mut slots)
    }

    /// Its value, whatever type (a sense).
    pub fn run_val(&self, env: &dyn Env) -> R<Val> {
        self.run(env)
    }

    fn run(&self, env: &dyn Env) -> R<Val> {
        let mut slots = vec![Val::I(0); self.slots];
        eval(&self.expr, env, &mut slots)
    }
}

fn effects_of(e: &Expr, env: &dyn Env, slots: &mut [Val]) -> R<Effects> {
    match e {
        Expr::Effects(list) => list
            .iter()
            .map(|(add, prop, v)| match eval(v, env, slots)? {
                Val::I(n) => Ok((*add, prop.clone(), n)),
                Val::B(_) => Err(Fallback),
            })
            .collect(),
        Expr::If(c, a, b) => {
            if truth(eval(c, env, slots)?)? {
                effects_of(a, env, slots)
            } else {
                effects_of(b, env, slots)
            }
        }
        Expr::Block(lets, v) => {
            for (slot, l) in lets {
                slots[*slot] = eval(l, env, slots)?;
            }
            effects_of(v, env, slots)
        }
        _ => Err(Fallback),
    }
}

fn truth(v: Val) -> R<bool> {
    match v {
        Val::B(b) => Ok(b),
        Val::I(_) => Err(Fallback),
    }
}

fn int(v: Val) -> R<i64> {
    match v {
        Val::I(i) => Ok(i),
        Val::B(_) => Err(Fallback),
    }
}

fn eval(e: &Expr, env: &dyn Env, slots: &mut [Val]) -> R<Val> {
    Ok(match e {
        Expr::Lit(v) => *v,
        Expr::Str(_) | Expr::Effects(_) => return Err(Fallback),
        Expr::Me(f) => Val::I(env.me(f).ok_or(Fallback)?),
        Expr::It(f) => Val::I(env.it(f).ok_or(Fallback)?),
        Expr::Arg(f) => Val::I(env.arg(f).ok_or(Fallback)?),
        Expr::Sense(f) => env.sense(f).ok_or(Fallback)?,
        Expr::Var(f) => Val::I(env.var(f).ok_or(Fallback)?),
        Expr::Local(i) => slots[*i],
        Expr::Neg(x) => Val::I(int(eval(x, env, slots)?)?.checked_neg().ok_or(Fallback)?),
        Expr::Not(x) => Val::B(!truth(eval(x, env, slots)?)?),
        Expr::Bin(Op::And, a, b) => Val::B(truth(eval(a, env, slots)?)? && truth(eval(b, env, slots)?)?),
        Expr::Bin(Op::Or, a, b) => Val::B(truth(eval(a, env, slots)?)? || truth(eval(b, env, slots)?)?),
        Expr::Bin(op, a, b) => {
            let (x, y) = (eval(a, env, slots)?, eval(b, env, slots)?);
            match (op, x, y) {
                (Op::Eq, Val::B(x), Val::B(y)) => Val::B(x == y),
                (Op::Ne, Val::B(x), Val::B(y)) => Val::B(x != y),
                (_, Val::I(x), Val::I(y)) => match op {
                    Op::Add => Val::I(x.checked_add(y).ok_or(Fallback)?),
                    Op::Sub => Val::I(x.checked_sub(y).ok_or(Fallback)?),
                    Op::Mul => Val::I(x.checked_mul(y).ok_or(Fallback)?),
                    Op::Div => Val::I(x.checked_div(y).ok_or(Fallback)?),
                    Op::Rem => Val::I(x.checked_rem(y).ok_or(Fallback)?),
                    Op::Lt => Val::B(x < y),
                    Op::Le => Val::B(x <= y),
                    Op::Gt => Val::B(x > y),
                    Op::Ge => Val::B(x >= y),
                    Op::Eq => Val::B(x == y),
                    Op::Ne => Val::B(x != y),
                    Op::And | Op::Or => unreachable!("handled above"),
                },
                _ => return Err(Fallback),
            }
        }
        Expr::If(c, a, b) => {
            if truth(eval(c, env, slots)?)? {
                eval(a, env, slots)?
            } else {
                eval(b, env, slots)?
            }
        }
        Expr::Block(lets, v) => {
            for (slot, l) in lets {
                slots[*slot] = eval(l, env, slots)?;
            }
            eval(v, env, slots)?
        }
        Expr::Call(f, args) => {
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(match a {
                    Expr::Str(s) => Arg::S(s.clone()),
                    x => Arg::V(eval(x, env, slots)?),
                });
            }
            match (f, vals.as_slice()) {
                (Func::Min, [Arg::V(Val::I(a)), Arg::V(Val::I(b))]) => Val::I(*a.min(b)),
                (Func::Max, [Arg::V(Val::I(a)), Arg::V(Val::I(b))]) => Val::I(*a.max(b)),
                (Func::Abs, [Arg::V(Val::I(a))]) => Val::I(a.checked_abs().ok_or(Fallback)?),
                (Func::Clamp, [Arg::V(Val::I(x)), Arg::V(Val::I(lo)), Arg::V(Val::I(hi))]) => Val::I((*x).max(*lo).min(*hi)),
                _ => env.call(*f, &vals).ok_or(Fallback)?,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct T;
    impl Env for T {
        fn me(&self, f: &str) -> Option<i64> {
            match f {
                "vy" => Some(120),
                "x" => Some(2),
                _ => None,
            }
        }
        fn it(&self, _: &str) -> Option<i64> {
            None
        }
        fn arg(&self, f: &str) -> Option<i64> {
            (f == "dx").then_some(-1)
        }
        fn sense(&self, f: &str) -> Option<Val> {
            (f == "beat").then_some(Val::B(true))
        }
        fn var(&self, f: &str) -> Option<i64> {
            (f == "roll").then_some(42)
        }
        fn call(&self, f: Func, args: &[Arg]) -> Option<Val> {
            match (f, args) {
                (Func::Ahead, [Arg::S(k)]) if k == "car" => Some(Val::I(3000)),
                _ => None,
            }
        }
    }

    fn p() -> BTreeMap<String, i64> {
        [("a".to_string(), 600), ("lanes".to_string(), 4)].into_iter().collect()
    }

    #[test]
    fn the_subset_evaluates_like_rhai() {
        let c = |s: &str| compile(s, &p(), 60).unwrap_or_else(|| panic!("compiles: {s}"));
        assert_eq!(c("me.vy * 100 / p.a + -3").int(&T), Ok(120 * 100 / 600 - 3));
        assert_eq!(c("me.x < p.lanes - 1 && sense.beat").bool(&T), Ok(true));
        assert_eq!(c("if me.x == 0 { 1 } else if me.x == 2 { 7 } else { 3 }").int(&T), Ok(7));
        assert_eq!(c("clamp(me.vy - 500, -p.a, p.a) + abs(-4) + min(1, 2) + max(1, 2)").int(&T), Ok(-380 + 4 + 1 + 2));
        assert_eq!(c("let s = ahead(\"car\"); let q = s * 2; q + arg.dx").int(&T), Ok(5999));
        assert_eq!(c("{ let a = 2; a * a }").int(&T), Ok(4));
        // A let whose value holds a block with lets of its own keeps its own slot (found by SIMCRAFT_NATIVE_CHECK).
        assert_eq!(c("let a = { let b = 3; let c = 4; b * c }; let d = 5; a + d").int(&T), Ok(17));
        assert_eq!(
            c("let s = 2100; let i = if s == 0 { 0 } else { let w = 4100; let q = w * 1000 / s; q * q / 1000 }; i").int(&T),
            Ok(3810)
        );
        assert_eq!(c("-7 / 2").int(&T), Ok(-3), "truncating, as Rhai");
        assert_eq!(c("-7 % 2").int(&T), Ok(-1));
        let s = c(r#"let v = me.vy; [ #{ op: "set", prop: "vy", value: v + 1 }, #{ op: "add", prop: "n", value: 2 } ]"#);
        assert_eq!(s.effects(&T), Ok(vec![(false, "vy".into(), 121), (true, "n".into(), 2)]));
    }

    #[test]
    fn anything_unexpected_falls_back_to_the_interpreter() {
        let c = |s: &str| compile(s, &p(), 60).unwrap();
        assert_eq!(c("me.vy / 0").int(&T), Err(Fallback), "division by zero: Rhai reports it");
        assert_eq!(c("me.missing + 1").int(&T), Err(Fallback), "a prop that is not there: Rhai reports it");
        assert_eq!(c("under(\"car\")").int(&T), Err(Fallback), "a call the host does not answer");
        assert_eq!(c("me.vy + sense.beat").int(&T), Err(Fallback), "types mixed: Rhai reports it");
        for not_subset in ["me.name == \"x\"", "p.unknown", "count.car", "x.len()", "me.vy ** 2", "near.car", "a | b"] {
            assert!(compile(not_subset, &p(), 60).is_none(), "not compiled: {not_subset}");
        }
    }
}
