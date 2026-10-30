//! A port of the SDK's `advanced/collect`: collects numbers and outputs them as a list on bang,
//! emptying the collection.
//!
//! The C++ original shows the STL and a mutex in an external; here they are a `Vec` and a
//! `Mutex`.

use std::sync::Mutex;

use median::prelude::*;

#[derive(MaxObj)]
#[max(name = "median.collect")]
pub struct Collect {
    values: Mutex<Vec<f64>>,
    out: Outlet<Any>,
}

impl MaxObjWrapped for Collect {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("input"));
        let mut values = Vec::with_capacity(10);
        values.extend(builder.creation_args().iter().map(Atom::get_float));
        Ok(Self {
            values: Mutex::new(values),
            out: builder.outlet(Outlet::any().assist("output")),
        })
    }
}

#[median::methods]
impl Collect {
    #[bang]
    pub fn bang(&self) {
        // Taken under one lock, so a number that arrives meanwhile is kept for the next bang;
        // the C unlocks between copying and clearing.
        let values = std::mem::take(&mut *self.values.lock().unwrap());
        if !values.is_empty() {
            let atoms: Vec<Atom> = values.into_iter().map(Atom::from).collect();
            let _ = self.out.send_list(&atoms);
        }
    }

    #[int]
    pub fn int(&self, v: t_atom_long) {
        self.float(v as f64);
    }

    #[float]
    pub fn float(&self, v: f64) {
        self.values.lock().unwrap().push(v);
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        self.values
            .lock()
            .unwrap()
            .extend(atoms.iter().map(Atom::get_float));
    }

    #[sel("clear")]
    pub fn clear(&self) {
        self.values.lock().unwrap().clear();
    }

    #[sel("count")]
    pub fn count(&self) {
        let count = self.values.lock().unwrap().len();
        let _ = self.out.send_int(count as t_atom_long);
    }
}

median::ext_main! { Collect }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn collects_until_bang() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.collect", &[1i64.into(), 2.5f64.into()]);
        assert!(!x.is_null());
        k.send_int(x, 3).unwrap();
        k.send_float(x, 4.5).unwrap();
        k.send_list(x, &[Atom::Long(5), Atom::Sym("six".into())])
            .unwrap();
        k.send(x, "count", &[]).unwrap();
        k.send_bang(x).unwrap();
        k.send_bang(x).unwrap();
        k.send(x, "count", &[]).unwrap();
        let out: Vec<_> = k
            .take_outlet_log()
            .into_iter()
            .map(|e| (e.selector, e.atoms))
            .collect();
        let floats = |v: &[f64]| v.iter().map(|&f| Atom::Float(f)).collect::<Vec<_>>();
        assert_eq!(
            out,
            [
                ("int".to_string(), vec![Atom::Long(6)]),
                ("list".to_string(), floats(&[1., 2.5, 3., 4.5, 5., 0.])),
                ("int".to_string(), vec![Atom::Long(0)]),
            ]
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn clear_empties() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.collect", &[]);
        k.send_int(x, 1).unwrap();
        k.send(x, "clear", &[]).unwrap();
        k.send_bang(x).unwrap();
        assert!(k.take_outlet_log().is_empty());
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("input"));
        assert_eq!(k.assist(x, true, 0).as_deref(), Some("output"));
        assert!(k.free_object(x));
    }
}
