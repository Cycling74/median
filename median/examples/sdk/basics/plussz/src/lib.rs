//! A port of the SDK's `basics/plussz`: two int inlets, their sum out on bang or a left int.

use median::prelude::*;

#[derive(MaxObj)]
#[max(name = "median.plussz")]
pub struct Plussz {
    left: Int64,
    right: Int64,
    sum: Outlet<Int>,
}

impl MaxObjWrapped for Plussz {
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        post!("plussz object loaded...");
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let right = builder.args().at(0).unwrap_or(0)?;
        builder.inlet(Inlet::default().assist("Inlet 0: Left Operand (Causes Output)"));
        builder.inlet(
            Inlet::int(|s: &Self, v| s.right.set(v))
                .assist("Inlet 1: Right Operand (Added to Left)"),
        );
        post!(" new plussz object instance added to patch...");
        Ok(Self {
            left: Int64::new(0),
            right: Int64::new(right),
            sum: builder.outlet(Outlet::int().assist("Sum of Left and Right Inlets")),
        })
    }
}

#[median::methods]
impl Plussz {
    #[bang]
    pub fn bang(&self) {
        let _ = self.sum.send(self.left.get() + self.right.get());
    }

    #[int]
    pub fn int(&self, v: t_atom_long) {
        self.left.set(v);
        self.bang();
    }
}

median::ext_main! { Plussz }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn sums_on_bang_and_left_int() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.plussz", &[5i64.into()]);
        assert!(!x.is_null());
        k.send_int(x, 2).unwrap();
        k.send_inlet(x, 1, "int", &[Atom::Long(10)]).unwrap();
        k.send_bang(x).unwrap();
        let out: Vec<_> = k.take_outlet_log().into_iter().map(|e| e.atoms).collect();
        assert_eq!(out, [vec![Atom::Long(7)], vec![Atom::Long(12)]]);
        assert_eq!(
            k.assist(x, true, 0).as_deref(),
            Some("Sum of Left and Right Inlets")
        );
        assert!(k.free_object(x));
    }
}
