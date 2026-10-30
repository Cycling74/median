//! A port of the SDK's `misc/match`: outputs the last few elements it received when they match
//! the arguments, where `nn` matches anything.

use std::sync::Mutex;

use median::{atom::AtomValue, prelude::*};

// `Atom` is not `Send`, so the kept sequences hold these instead.
#[derive(Clone, Copy)]
enum Value {
    Int(t_atom_long),
    Float(f64),
    Sym(SymbolRef),
}

impl Value {
    fn of(a: &Atom) -> Option<Self> {
        match a.get_value()? {
            AtomValue::Int(i) => Some(Value::Int(i)),
            AtomValue::Float(f) => Some(Value::Float(f)),
            AtomValue::Symbol(s) => Some(Value::Sym(s)),
            AtomValue::Object(_) => None,
        }
    }

    // `self` was received, `want` is wanted.
    fn matches(self, want: Value) -> bool {
        use Value::*;
        match (self, want) {
            (_, Sym(w)) if w == median::sym!("nn") => true,
            (Sym(a), Sym(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Float(f), Int(i)) | (Int(i), Float(f)) => {
                f == f as t_atom_long as f64 && f as t_atom_long == i
            }
            _ => false,
        }
    }
}

impl From<Value> for Atom {
    fn from(v: Value) -> Self {
        match v {
            Value::Int(i) => i.into(),
            Value::Float(f) => f.into(),
            Value::Sym(s) => s.into(),
        }
    }
}

struct State {
    want: Vec<Value>,
    // As long as `want`; `None` for an element not received since the last `set` or `clear`.
    // `nn` does not match `None`: the SDK's does, but then outputs nothing, as the match starts
    // with an element it never received.
    seen: Vec<Option<Value>>,
}

#[derive(MaxObj)]
#[max(name = "median.match")]
pub struct Match {
    state: Mutex<State>,
    out: Outlet<Any>,
}

impl MaxObjWrapped for Match {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("Input Sequence to be Matched"));
        let want: Vec<Value> = builder.args().raw().iter().filter_map(Value::of).collect();
        Ok(Self {
            state: Mutex::new(State {
                seen: vec![None; want.len()],
                want,
            }),
            out: builder.outlet(Outlet::any().assist("list of Matched Sequence")),
        })
    }
}

#[median::methods]
impl Match {
    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.value(Value::Int(n));
    }

    #[float]
    pub fn float(&self, f: f64) {
        self.value(Value::Float(f));
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        for v in atoms.iter().filter_map(Value::of) {
            self.value(v);
        }
    }

    #[any]
    pub fn anything(&self, sel: &SymbolRef, atoms: &[Atom]) {
        self.value(Value::Sym(*sel));
        self.list(atoms);
    }

    #[gimme("set")]
    pub fn set(&self, atoms: &[Atom]) {
        let want: Vec<Value> = atoms.iter().filter_map(Value::of).collect();
        if want.is_empty() {
            return;
        }
        let mut state = self.state.lock().unwrap();
        state.seen = vec![None; want.len()];
        state.want = want;
    }

    #[sel("clear")]
    pub fn clear(&self) {
        self.state.lock().unwrap().seen.fill(None);
    }

    fn value(&self, v: Value) {
        let matched: Vec<Atom> = {
            let mut state = self.state.lock().unwrap();
            if state.seen.is_empty() {
                return;
            }
            state.seen.remove(0);
            state.seen.push(Some(v));
            let State { want, seen } = &*state;
            if !seen
                .iter()
                .zip(want)
                .all(|(s, &w)| s.is_some_and(|s| s.matches(w)))
            {
                return;
            }
            seen.iter().flatten().map(|&v| v.into()).collect()
        };
        let _ = match &matched[..] {
            [single] => match single.get_value() {
                Some(AtomValue::Int(i)) => self.out.send_int(i),
                Some(AtomValue::Float(f)) => self.out.send_float(f),
                Some(AtomValue::Symbol(s)) => self.out.send(&s, &[]),
                _ => Ok(()),
            },
            [first, rest @ ..] => match first.get_value() {
                Some(AtomValue::Symbol(s)) => self.out.send(&s, rest),
                _ => self.out.send_list(&matched),
            },
            [] => Ok(()),
        };
    }
}

median::ext_main! { Match }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn outputs(k: &median_mock::Kernel) -> Vec<(String, Vec<Atom>)> {
        k.take_outlet_log()
            .into_iter()
            .map(|e| (e.selector, e.atoms))
            .collect()
    }

    #[test]
    fn matches_a_sequence_with_wildcards() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object(
            "median.match",
            &[Atom::Long(1), Atom::Sym("nn".into()), Atom::Float(3.)],
        );
        assert!(!x.is_null());
        for v in [5, 1, 9] {
            k.send_int(x, v).unwrap();
        }
        k.send_int(x, 3).unwrap();
        assert_eq!(
            outputs(&k),
            [(
                "list".to_string(),
                vec![Atom::Long(1), Atom::Long(9), Atom::Long(3)]
            )]
        );
        k.send_list(
            x,
            &[Atom::Float(1.), Atom::Sym("x".into()), Atom::Float(3.)],
        )
        .unwrap();
        assert_eq!(outputs(&k).len(), 1);
        k.send_list(x, &[Atom::Float(1.5), Atom::Long(0), Atom::Long(3)])
            .unwrap();
        assert!(outputs(&k).is_empty());
        assert!(k.free_object(x));
    }

    #[test]
    fn set_and_clear() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.match", &[]);
        k.send_int(x, 1).unwrap();
        assert!(outputs(&k).is_empty());
        k.send(x, "set", &[Atom::Sym("go".into()), Atom::Long(2)])
            .unwrap();
        k.send(x, "go", &[Atom::Long(2)]).unwrap();
        assert_eq!(outputs(&k), [("go".to_string(), vec![Atom::Long(2)])]);
        k.send(x, "go", &[]).unwrap();
        k.send(x, "clear", &[]).unwrap();
        k.send_int(x, 2).unwrap();
        assert!(outputs(&k).is_empty());
        k.send(x, "set", &[Atom::Sym("go".into())]).unwrap();
        k.send(x, "go", &[]).unwrap();
        assert_eq!(outputs(&k), [("go".to_string(), vec![])]);
        assert_eq!(
            k.assist(x, false, 0).as_deref(),
            Some("Input Sequence to be Matched")
        );
        assert!(k.free_object(x));
    }
}
