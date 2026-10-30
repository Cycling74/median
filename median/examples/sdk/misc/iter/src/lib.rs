//! A port of the SDK's `misc/iter`: outputs the elements of a list one at a time; bang repeats
//! the last one.

use std::sync::Mutex;

use median::{atom::AtomValue, prelude::*};

// `Atom` is not `Send`, so the kept list holds these instead.
#[derive(Clone, Copy)]
enum Value {
    Int(t_atom_long),
    Float(f64),
    Sym(SymbolRef),
}

fn values(atoms: &[Atom]) -> Vec<Value> {
    atoms
        .iter()
        .filter_map(|a| match a.get_value() {
            Some(AtomValue::Int(i)) => Some(Value::Int(i)),
            Some(AtomValue::Float(f)) => Some(Value::Float(f)),
            Some(AtomValue::Symbol(s)) => Some(Value::Sym(s)),
            _ => None,
        })
        .collect()
}

#[derive(MaxObj)]
#[max(name = "median.iter")]
pub struct Iter {
    last: Mutex<Vec<Value>>,
    out: Outlet<Any>,
}

impl MaxObjWrapped for Iter {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("list to be Unraveled"));
        Ok(Self {
            last: Mutex::new(Vec::new()),
            out: builder.outlet(Outlet::any().assist("Sequential Output of Incoming list")),
        })
    }
}

#[median::methods]
impl Iter {
    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.store_and_output(vec![Value::Int(n)]);
    }

    #[float]
    pub fn float(&self, f: f64) {
        self.store_and_output(vec![Value::Float(f)]);
    }

    #[bang]
    pub fn bang(&self) {
        // Copied out so the lock is not held while the output may come back to this object.
        let last = self.last.lock().unwrap().clone();
        self.output(&last);
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        self.store_and_output(values(atoms));
    }

    #[any]
    pub fn anything(&self, sel: &SymbolRef, atoms: &[Atom]) {
        let mut message = vec![Value::Sym(*sel)];
        message.extend(values(atoms));
        self.store_and_output(message);
    }

    fn store_and_output(&self, values: Vec<Value>) {
        self.last.lock().unwrap().clone_from(&values);
        self.output(&values);
    }

    fn output(&self, values: &[Value]) {
        for v in values {
            let _ = match *v {
                Value::Int(i) => self.out.send_int(i),
                Value::Float(f) => self.out.send_float(f),
                Value::Sym(s) => self.out.send(&s, &[]),
            };
        }
    }
}

median::ext_main! { Iter }

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
    fn unravels_lists_and_repeats_on_bang() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.iter", &[]);
        assert!(!x.is_null());
        k.send_list(
            x,
            &[Atom::Long(1), Atom::Float(2.5), Atom::Sym("three".into())],
        )
        .unwrap();
        let expected = vec![
            ("int".to_string(), vec![Atom::Long(1)]),
            ("float".to_string(), vec![Atom::Float(2.5)]),
            ("three".to_string(), vec![]),
        ];
        assert_eq!(outputs(&k), expected);
        k.send_bang(x).unwrap();
        assert_eq!(outputs(&k), expected);
        k.send_int(x, 7).unwrap();
        k.send_bang(x).unwrap();
        assert_eq!(outputs(&k).len(), 2);
        assert!(k.free_object(x));
    }

    #[test]
    fn anything_outputs_its_selector_first() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.iter", &[]);
        k.send(x, "foo", &[Atom::Long(4)]).unwrap();
        k.send_bang(x).unwrap();
        let one = vec![
            ("foo".to_string(), vec![]),
            ("int".to_string(), vec![Atom::Long(4)]),
        ];
        assert_eq!(outputs(&k), [one.clone(), one].concat());
        assert_eq!(
            k.assist(x, true, 0).as_deref(),
            Some("Sequential Output of Incoming list")
        );
        assert!(k.free_object(x));
    }
}
