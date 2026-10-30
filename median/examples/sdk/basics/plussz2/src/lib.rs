//! A port of the SDK's `basics/plussz2`: `plussz` with a proxy right inlet that takes ints and
//! floats; the sum is an int when both operands are, otherwise a float.

use std::sync::Mutex;

use median::prelude::*;

#[derive(Clone, Copy)]
enum Operand {
    Int(t_atom_long),
    Float(f64),
}

impl Operand {
    fn float(self) -> f64 {
        match self {
            Operand::Int(i) => i as f64,
            Operand::Float(f) => f,
        }
    }
}

#[derive(MaxObj)]
#[max(name = "median.plussz2")]
pub struct Plussz2 {
    obj: ObjRef,
    // Left, right.
    operands: Mutex<[Operand; 2]>,
    sum: Outlet<Any>,
}

impl MaxObjWrapped for Plussz2 {
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        post!("plussz2 object loaded...");
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("Inlet 0: Left Operand (Causes Output)"));
        builder.inlet(Inlet::proxy().assist("Inlet 1: Right Operand (Added to Left)"));
        post!(" new plussz2 object instance added to patch...");
        Ok(Self {
            obj: builder.obj(),
            operands: Mutex::new([Operand::Int(0); 2]),
            sum: builder.outlet(Outlet::any().assist("Sum of Left and Right Inlets")),
        })
    }
}

#[median::methods]
impl Plussz2 {
    #[bang]
    pub fn bang(&self) {
        let [l, r] = *self.operands.lock().unwrap();
        // The SDK sums floats in a C `float`; this keeps the double.
        let _ = match (l, r) {
            (Operand::Int(l), Operand::Int(r)) => self.sum.send_int(l.wrapping_add(r)),
            _ => self.sum.send_float(l.float() + r.float()),
        };
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        let inlet = self.obj.proxy_inlet();
        post!("int came in via inlet {inlet}");
        self.store(inlet, Operand::Int(n));
    }

    #[float]
    pub fn float(&self, f: f64) {
        let inlet = self.obj.proxy_inlet();
        post!("float came in via inlet {inlet}");
        self.store(inlet, Operand::Float(f));
    }

    fn store(&self, inlet: usize, v: Operand) {
        if inlet == 1 {
            self.operands.lock().unwrap()[1] = v;
        } else {
            self.operands.lock().unwrap()[0] = v;
            self.bang();
        }
    }
}

median::ext_main! { Plussz2 }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn sums_ints_and_floats_by_inlet() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.plussz2", &[]);
        assert!(!x.is_null());
        k.take_console_log();
        k.send_inlet(x, 1, "int", &[Atom::Long(10)]).unwrap();
        k.send_int(x, 2).unwrap();
        k.send_inlet(x, 1, "float", &[Atom::Float(0.5)]).unwrap();
        k.send_bang(x).unwrap();
        k.send_float(x, 1.25).unwrap();
        let out: Vec<_> = k.take_outlet_log().into_iter().map(|e| e.atoms).collect();
        assert_eq!(
            out,
            [
                vec![Atom::Long(12)],
                vec![Atom::Float(2.5)],
                vec![Atom::Float(1.75)]
            ]
        );
        let console: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(
            console,
            [
                "int came in via inlet 1",
                "int came in via inlet 0",
                "float came in via inlet 1",
                "float came in via inlet 0",
            ]
        );
        assert_eq!(
            k.assist(x, false, 1).as_deref(),
            Some("Inlet 1: Right Operand (Added to Left)")
        );
        assert!(k.free_object(x));
    }
}
