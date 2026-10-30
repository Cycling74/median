//! A port of the SDK's `misc/minimum`: outputs the smallest of a list, or of its two inlets, and
//! its index. A float argument makes the right inlet and the output floats.

use std::sync::Mutex;

use median::{atom::AtomValue, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Num {
    Int(t_atom_long),
    Float(f64),
}

impl Num {
    fn as_f64(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(f) => f,
        }
    }
}

enum MinOutlet {
    Int(Outlet<Int>),
    Float(Outlet<Float>),
}

#[derive(MaxObj)]
#[max(name = "median.minimum")]
pub struct Minimum {
    // Never empty: the left inlet's value is the first element.
    args: Mutex<Vec<Num>>,
    min: MinOutlet,
    index: Outlet<Int>,
}

impl MaxObjWrapped for Minimum {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("Compares Left and Right Inlets"));
        let right = match builder.args().at(0).atom().and_then(Atom::get_value) {
            Some(AtomValue::Float(f)) => {
                builder.inlet(
                    Inlet::float(|s: &Self, f| s.right(Num::Float(f)))
                        .assist("Value to be Compared"),
                );
                Num::Float(f)
            }
            v => {
                builder.inlet(
                    Inlet::int(|s: &Self, n| s.right(Num::Int(n))).assist("Value to be Compared"),
                );
                match v {
                    Some(AtomValue::Int(i)) => Num::Int(i),
                    _ => Num::Int(0),
                }
            }
        };
        let (first, min) = match right {
            Num::Float(_) => (
                Num::Float(0.),
                MinOutlet::Float(builder.outlet(Outlet::float().assist("Minimum Value"))),
            ),
            Num::Int(_) => (
                Num::Int(0),
                MinOutlet::Int(builder.outlet(Outlet::int().assist("Minimum Value"))),
            ),
        };
        Ok(Self {
            args: Mutex::new(vec![first, right]),
            min,
            index: builder.outlet(Outlet::int().assist("Index of the Minimum Value")),
        })
    }
}

#[median::methods]
impl Minimum {
    #[bang]
    pub fn bang(&self) {
        let (min, index) = minimum(&self.args.lock().unwrap());
        let _ = self.index.send(index as t_atom_long);
        let _ = match &self.min {
            MinOutlet::Int(o) => o.send(match min {
                Num::Int(i) => i,
                Num::Float(f) => f as t_atom_long,
            }),
            MinOutlet::Float(o) => o.send(min.as_f64()),
        };
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.left(Num::Int(n));
    }

    #[float]
    pub fn float(&self, f: f64) {
        self.left(Num::Float(f));
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        if atoms.is_empty() {
            return;
        }
        // The SDK leaves a symbol's slot uninitialised; it counts as 0 here.
        *self.args.lock().unwrap() = atoms
            .iter()
            .map(|a| match a.get_value() {
                Some(AtomValue::Float(f)) => Num::Float(f),
                Some(AtomValue::Int(i)) => Num::Int(i),
                _ => Num::Int(0),
            })
            .collect();
        self.bang();
    }

    fn left(&self, v: Num) {
        self.args.lock().unwrap()[0] = v;
        self.bang();
    }

    fn right(&self, v: Num) {
        let mut args = self.args.lock().unwrap();
        args.truncate(1);
        args.push(v);
    }
}

// The smallest value and its index. Values compare as floats unless both are ints; the first of
// equal values wins.
fn minimum(args: &[Num]) -> (Num, usize) {
    let mut min = args[0];
    let mut index = 0;
    for (i, &v) in args.iter().enumerate().skip(1) {
        let smaller = match (min, v) {
            (Num::Int(a), Num::Int(b)) => b < a,
            _ => v.as_f64() < min.as_f64(),
        };
        if smaller {
            min = v;
            index = i;
        }
    }
    (min, index)
}

median::ext_main! { Minimum }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn outputs(k: &median_mock::Kernel) -> Vec<(usize, Vec<Atom>)> {
        k.take_outlet_log()
            .into_iter()
            .map(|e| (e.index, e.atoms))
            .collect()
    }

    #[test]
    fn int_minimum_of_both_inlets() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.minimum", &[4i64.into()]);
        assert!(!x.is_null());
        k.send_int(x, 7).unwrap();
        assert_eq!(
            outputs(&k),
            [(1, vec![Atom::Long(1)]), (0, vec![Atom::Long(4)])]
        );
        k.send_inlet(x, 1, "int", &[Atom::Long(9)]).unwrap();
        assert!(outputs(&k).is_empty());
        k.send_bang(x).unwrap();
        assert_eq!(
            outputs(&k),
            [(1, vec![Atom::Long(0)]), (0, vec![Atom::Long(7)])]
        );
        k.send_list(x, &[Atom::Long(3), Atom::Float(2.5), Atom::Long(1)])
            .unwrap();
        assert_eq!(
            outputs(&k),
            [(1, vec![Atom::Long(2)]), (0, vec![Atom::Long(1)])]
        );
        k.send_float(x, 0.5).unwrap();
        assert_eq!(
            outputs(&k),
            [(1, vec![Atom::Long(0)]), (0, vec![Atom::Long(0)])]
        );
        assert_eq!(k.outlet_types(x), [Some("int".into()), Some("int".into())]);
        assert!(k.free_object(x));
    }

    #[test]
    fn float_argument_makes_float_io() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.minimum", &[Atom::Float(1.5)]);
        k.send_inlet(x, 1, "float", &[Atom::Float(-2.25)]).unwrap();
        k.send_int(x, 3).unwrap();
        assert_eq!(
            outputs(&k),
            [(1, vec![Atom::Long(1)]), (0, vec![Atom::Float(-2.25)])]
        );
        assert_eq!(
            k.outlet_types(x),
            [Some("float".into()), Some("int".into())]
        );
        assert_eq!(
            k.assist(x, true, 1).as_deref(),
            Some("Index of the Minimum Value")
        );
        assert_eq!(
            k.assist(x, false, 1).as_deref(),
            Some("Value to be Compared")
        );
        assert!(k.free_object(x));
    }
}
