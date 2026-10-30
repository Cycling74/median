//! A port of the SDK's `misc/vexpr`: evaluates an `expr` expression element by element over
//! lists, with an inlet for each `$i`, `$f` or `$s` variable.
//!
//! Deviations: for an expression that skips a variable (`$i1 + $i3`) the C makes no inlet for the
//! skipped one but still counts its (never set) list, so it never outputs; here only variables
//! with an inlet count. With an empty input the C's scalar mode outputs an uninitialized result;
//! here it outputs nothing. `scalarmode` is a `long` attribute rather than a `char`. median-mock
//! has no `expr`, so no object can be made in the tests: they cover registration and the
//! evaluation loop, with a stand-in for `expr_eval`.

use std::{
    ffi::{c_long, c_void},
    ptr::NonNull,
    sync::Mutex,
};

use median::{
    atom::{AtomType, AtomValue},
    max_sys,
    prelude::*,
};

const MAXLIST: usize = 4096;
const MAXVARS: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Val {
    Int(t_atom_long),
    Float(f64),
    Sym(SymbolRef),
}

impl Val {
    fn from_atom(a: &Atom) -> Option<Self> {
        match a.get_value()? {
            AtomValue::Int(i) => Some(Self::Int(i)),
            AtomValue::Float(f) => Some(Self::Float(f)),
            AtomValue::Symbol(s) => Some(Self::Sym(s)),
            AtomValue::Object(_) => None,
        }
    }

    fn atom(self) -> Atom {
        match self {
            Self::Int(i) => i.into(),
            Self::Float(f) => f.into(),
            Self::Sym(s) => s.into(),
        }
    }
}

struct Input {
    // The variable: 0 for `$i1`, `$f1` or `$s1`.
    var: usize,
    // What `expr_new` says the variable is: `Int`, `Float`, or `Symbol` for a table.
    ty: Option<AtomType>,
    vals: Vec<Val>,
}

struct Expr(NonNull<c_void>);

// SAFETY: the expr object is only used by the thread holding the `State` lock.
unsafe impl Send for Expr {}

impl Drop for Expr {
    fn drop(&mut self) {
        // SAFETY: the object came from `expr_new` and is freed only here.
        unsafe { max_sys::freeobject(self.0.as_ptr().cast()) };
    }
}

impl Expr {
    fn eval(&self, args: &[(usize, Val)]) -> Option<Val> {
        let mut argv: [Atom; MAXVARS] = std::array::from_fn(|_| nothing());
        let argc = args.iter().map(|&(var, _)| var + 1).max().unwrap_or(1);
        for &(var, val) in args {
            argv[var] = val.atom();
        }
        let mut result = nothing();
        // SAFETY: the expr object is live and used by this thread only; `argv` has `argc` atoms
        // and `result` is writable. `Atom` is a transparent `t_atom`.
        // GAP: expr_eval
        unsafe {
            max_sys::expr_eval(
                self.0.as_ptr().cast(),
                argc as c_long,
                argv.as_mut_ptr().cast(),
                (&raw mut result).cast(),
            )
        };
        Val::from_atom(&result)
    }
}

// What `expr` expects for an unused variable.
fn nothing() -> Atom {
    // SAFETY: `Atom` is a transparent `t_atom`, for which all zeroes is a valid `A_NOTHING` atom.
    unsafe { std::mem::zeroed() }
}

struct State {
    expr: Expr,
    // One per inlet, left to right.
    inputs: Vec<Input>,
}

#[derive(MaxObj)]
#[max(name = "median.vexpr")]
pub struct Vexpr {
    obj: ObjRef,
    state: Mutex<State>,
    #[attr(style = "onoff", label = "Scalar Mode", category = "Behavior")]
    scalarmode: Int64,
    out: Outlet<Any>,
}

impl MaxObjWrapped for Vexpr {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let mut args: Vec<Atom> = builder
            .args()
            .positional()
            .iter()
            .filter_map(Val::from_atom)
            .map(Val::atom)
            .collect();
        let mut types: [Atom; MAXVARS] = std::array::from_fn(|_| nothing());
        // SAFETY: `args` has `args.len()` atoms and `types` the nine `expr_new` fills in.
        // GAP: expr_new
        let expr = unsafe {
            max_sys::expr_new(
                args.len() as c_long,
                args.as_mut_ptr().cast(),
                types.as_mut_ptr().cast(),
            )
        };
        let expr = Expr(NonNull::new(expr).ok_or("could not make the expression")?);
        builder
            .inlet(Inlet::default().assist("Evaluate Expression, int $i1, float $f1, table $s1"));
        let mut inputs = vec![Input {
            var: 0,
            ty: types[0].get_type(),
            vals: Vec::new(),
        }];
        for (var, ty) in types.iter().enumerate().skip(1) {
            let Some(ty) = ty.get_type() else { continue };
            let n = var + 1;
            builder.inlet(Inlet::proxy().assist(&format!("int $i{n}, float $f{n}, table $s{n}")));
            inputs.push(Input {
                var,
                ty: Some(ty),
                vals: Vec::new(),
            });
        }
        Ok(Self {
            obj: builder.obj(),
            state: Mutex::new(State { expr, inputs }),
            scalarmode: Int64::new(0),
            out: builder.outlet(Outlet::any().assist("Expression Result")),
        })
    }
}

#[median::methods]
impl Vexpr {
    #[bang]
    pub fn bang(&self) {
        let results = {
            let state = self.state.lock().unwrap();
            evaluate(&state.inputs, self.scalarmode.get() != 0, |args| {
                state.expr.eval(args)
            })
        };
        let _ = match results[..] {
            [] => Ok(()),
            [Val::Int(i)] => self.out.send_int(i),
            [Val::Float(f)] => self.out.send_float(f),
            [Val::Sym(_)] => Ok(()),
            _ => self
                .out
                .send_list(&results.into_iter().map(Val::atom).collect::<Vec<_>>()),
        };
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.store(|ty| match ty {
            Some(AtomType::Float) => vec![Val::Float(n as f64)],
            _ => vec![Val::Int(n)],
        });
    }

    #[float]
    pub fn float(&self, f: f64) {
        self.store(|ty| match ty {
            Some(AtomType::Int) => vec![Val::Int(f as t_atom_long)],
            _ => vec![Val::Float(f)],
        });
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        let atoms = &atoms[..atoms.len().min(MAXLIST)];
        self.store(|ty| {
            atoms
                .iter()
                .map(|a| match ty {
                    Some(AtomType::Int) => Val::Int(a.get_int()),
                    Some(AtomType::Float) => Val::Float(a.get_float()),
                    _ => Val::from_atom(a).unwrap_or(Val::Int(0)),
                })
                .collect()
        });
    }

    /// Set the variable of the inlet the message came in, from its type; the leftmost inlet
    /// evaluates.
    fn store(&self, vals: impl FnOnce(Option<AtomType>) -> Vec<Val>) {
        let inlet = self.obj.proxy_inlet();
        {
            let mut state = self.state.lock().unwrap();
            let Some(input) = state.inputs.get_mut(inlet) else {
                return;
            };
            input.vals = vals(input.ty);
        }
        if inlet == 0 {
            self.bang();
        }
    }
}

/// Evaluate the expression for each element, as `(variable, value)` arguments to `eval`. In
/// scalar mode a one-element input is used for every element; otherwise the shortest input sets
/// the count.
fn evaluate(
    inputs: &[Input],
    scalar: bool,
    mut eval: impl FnMut(&[(usize, Val)]) -> Option<Val>,
) -> Vec<Val> {
    let is_scalar = |input: &Input| scalar && input.vals.len() == 1;
    let count = inputs
        .iter()
        .filter(|i| !is_scalar(i))
        .map(|i| i.vals.len())
        .min()
        // Every input is a scalar.
        .unwrap_or(1)
        .min(MAXLIST);
    let mut args = Vec::with_capacity(inputs.len());
    let mut results = Vec::with_capacity(count);
    for n in 0..count {
        args.clear();
        args.extend(inputs.iter().map(|i| {
            let index = if is_scalar(i) { 0 } else { n };
            (i.var, i.vals[index])
        }));
        if let Some(v) = eval(&args) {
            results.push(v);
        }
    }
    results
}

median::ext_main! { Vexpr }

#[cfg(test)]
mod tests {
    use super::{AtomType, Input, Val, evaluate};

    fn input(var: usize, vals: &[i64]) -> Input {
        Input {
            var,
            ty: Some(AtomType::Int),
            vals: vals.iter().map(|&v| Val::Int(v as _)).collect(),
        }
    }

    fn sum(args: &[(usize, Val)]) -> Option<Val> {
        let mut total = 0;
        for &(var, v) in args {
            let Val::Int(i) = v else { return None };
            total += i * (var as isize + 1);
        }
        Some(Val::Int(total))
    }

    #[test]
    fn registers() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        assert!(k.class_exists("median.vexpr"));
    }

    #[test]
    fn vector_mode_stops_at_the_shortest_list() {
        let inputs = [input(0, &[1, 2, 3]), input(2, &[10, 20])];
        assert_eq!(evaluate(&inputs, false, sum), [Val::Int(31), Val::Int(62)]);
        let inputs = [input(0, &[1, 2, 3]), input(1, &[])];
        assert!(evaluate(&inputs, false, sum).is_empty());
    }

    #[test]
    fn scalar_mode_repeats_single_values() {
        let inputs = [input(0, &[1, 2, 3]), input(1, &[10])];
        assert_eq!(
            evaluate(&inputs, true, sum),
            [Val::Int(21), Val::Int(22), Val::Int(23)]
        );
        let inputs = [input(0, &[4]), input(1, &[5])];
        assert_eq!(evaluate(&inputs, true, sum), [Val::Int(14)]);
        let inputs = [input(0, &[4]), input(1, &[])];
        assert!(evaluate(&inputs, true, sum).is_empty());
        let inputs = [input(0, &[4]), input(1, &[5])];
        assert!(evaluate(&inputs, false, sum).len() == 1);
    }
}
