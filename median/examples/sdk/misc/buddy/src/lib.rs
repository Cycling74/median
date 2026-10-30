//! A port of the SDK's `misc/buddy`: holds a message from each of its inlets and, once every inlet
//! has received one, sends them all out of the matching outlets, right to left.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use median::{atom::AtomValue, prelude::*};

// The SDK keeps 128 atoms per inlet: the selector and at most 127 arguments.
const MAX_ARGS: usize = 127;

// `Atom` is not `Send`, so the kept messages hold these instead.
#[derive(Clone, Copy)]
enum Value {
    Int(t_atom_long),
    Float(f64),
    Sym(SymbolRef),
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

#[derive(Clone)]
enum Message {
    // An int, a float, or a symbol sent as a message without arguments.
    Single(Value),
    Selector(SymbolRef, Vec<Value>),
}

struct Member {
    on: AtomicBool,
    message: Mutex<Message>,
    out: Outlet<Any>,
}

#[derive(MaxObj)]
#[max(name = "median.buddy")]
pub struct Buddy {
    obj: ObjRef,
    members: Vec<Member>,
}

impl MaxObjWrapped for Buddy {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let num: t_atom_long = builder.args().at(0).unwrap_or(0)?;
        let num = num.max(2) as usize;
        builder.inlet(Inlet::default().assist("Input to be Synchronized"));
        for _ in 1..num {
            builder.inlet(Inlet::proxy().assist("Input to be Synchronized"));
        }
        let members = (0..num)
            .map(|i| Member {
                on: AtomicBool::new(false),
                message: Mutex::new(Message::Single(Value::Int(0))),
                out: builder.outlet(
                    Outlet::any().assist(&format!("Synchronized Output of Inlet {}", i + 1)),
                ),
            })
            .collect();
        Ok(Self {
            obj: builder.obj(),
            members,
        })
    }

    // An inlet is hot when it is the only one still waiting.
    fn inlet_hot(&self, index: usize) -> Option<bool> {
        let on = self
            .members
            .iter()
            .filter(|m| m.on.load(Ordering::Relaxed))
            .count();
        let waiting = !self.members.get(index)?.on.load(Ordering::Relaxed);
        Some(on + 1 >= self.members.len() && waiting)
    }
}

#[median::methods]
impl Buddy {
    #[bang]
    pub fn bang(&self) {
        self.int(0);
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.store(Message::Single(Value::Int(n)));
    }

    #[float]
    pub fn float(&self, f: f64) {
        self.store(Message::Single(Value::Float(f)));
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        self.anything(&median::sym!("list"), atoms);
    }

    #[any]
    pub fn anything(&self, sel: &SymbolRef, atoms: &[Atom]) {
        if atoms.is_empty() {
            self.store(Message::Single(Value::Sym(*sel)));
            return;
        }
        let args = atoms
            .iter()
            .take(MAX_ARGS)
            .filter_map(|a| match a.get_value() {
                Some(AtomValue::Int(i)) => Some(Value::Int(i)),
                Some(AtomValue::Float(f)) => Some(Value::Float(f)),
                Some(AtomValue::Symbol(s)) => Some(Value::Sym(s)),
                _ => None,
            })
            .collect();
        self.store(Message::Selector(*sel, args));
    }

    #[sel("clear")]
    pub fn clear(&self) {
        for m in &self.members {
            m.on.store(false, Ordering::Relaxed);
        }
    }

    fn store(&self, message: Message) {
        let Some(m) = self.members.get(self.obj.proxy_inlet()) else {
            return;
        };
        *m.message.lock().unwrap() = message;
        m.on.store(true, Ordering::Relaxed);
        if self.members.iter().all(|m| m.on.load(Ordering::Relaxed)) {
            self.clear();
            self.output();
        }
    }

    fn output(&self) {
        for m in self.members.iter().rev() {
            // Copied out so the lock is not held while the output may come back to this object.
            let message = m.message.lock().unwrap().clone();
            let _ = match message {
                Message::Single(Value::Int(i)) => m.out.send_int(i),
                Message::Single(Value::Float(f)) => m.out.send_float(f),
                Message::Single(Value::Sym(s)) => m.out.send(&s, &[]),
                Message::Selector(sel, args) => {
                    let args: Vec<Atom> = args.into_iter().map(Atom::from).collect();
                    if sel == median::sym!("list") {
                        m.out.send_list(&args)
                    } else {
                        m.out.send(&sel, &args)
                    }
                }
            };
        }
    }
}

median::ext_main! { Buddy }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn outputs_once_every_inlet_has_a_message() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.buddy", &[3i64.into()]);
        assert!(!x.is_null());
        assert_eq!(k.outlet_count(x), 3);
        k.send_inlet(x, 2, "foo", &[Atom::Long(1), Atom::Sym("bar".into())])
            .unwrap();
        k.send_int(x, 5).unwrap();
        assert!(k.take_outlet_log().is_empty());
        assert_eq!(k.inlet_is_hot(x, 1), Some(true));
        assert_eq!(k.inlet_is_hot(x, 0), Some(false));
        k.send_inlet(x, 1, "list", &[Atom::Float(0.5), Atom::Long(2)])
            .unwrap();
        let out: Vec<_> = k
            .take_outlet_log()
            .into_iter()
            .map(|e| (e.index, e.selector, e.atoms))
            .collect();
        assert_eq!(
            out,
            [
                (
                    2,
                    "foo".to_string(),
                    vec![Atom::Long(1), Atom::Sym("bar".into())]
                ),
                (1, "list".to_string(), vec![Atom::Float(0.5), Atom::Long(2)]),
                (0, "int".to_string(), vec![Atom::Long(5)]),
            ]
        );
        assert_eq!(k.inlet_is_hot(x, 0), Some(false));
        assert!(k.free_object(x));
    }

    #[test]
    fn clear_forgets_and_bang_is_zero() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.buddy", &[]);
        assert_eq!(k.outlet_count(x), 2);
        k.send_float(x, 1.5).unwrap();
        k.send(x, "clear", &[]).unwrap();
        k.send_inlet(x, 1, "baz", &[]).unwrap();
        assert!(k.take_outlet_log().is_empty());
        k.send_bang(x).unwrap();
        let out: Vec<_> = k
            .take_outlet_log()
            .into_iter()
            .map(|e| (e.selector, e.atoms))
            .collect();
        assert_eq!(
            out,
            [
                ("baz".to_string(), vec![]),
                ("int".to_string(), vec![Atom::Long(0)])
            ]
        );
        assert_eq!(
            k.assist(x, true, 1).as_deref(),
            Some("Synchronized Output of Inlet 2")
        );
        assert_eq!(
            k.assist(x, false, 1).as_deref(),
            Some("Input to be Synchronized")
        );
        assert!(k.free_object(x));
    }
}
