//! A port of the SDK's `misc/past`: bang once when the input rises past the arguments.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

use median::{atom::AtomValue, prelude::*};

const MAX_SIZE: usize = 8;

#[derive(MaxObj)]
#[max(name = "median.past")]
pub struct Past {
    threshold: Mutex<Vec<Option<f64>>>,
    // Set once the input has risen past the threshold; cleared when it falls back.
    set: AtomicBool,
    out: Outlet<Bang>,
}

impl MaxObjWrapped for Past {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("Input to be Synchronized"));
        Ok(Self {
            threshold: Mutex::new(numbers(builder.args().raw())),
            set: AtomicBool::new(false),
            out: builder
                .outlet(Outlet::bang().assist("bang When Numbers Rise Above Specified Value")),
        })
    }
}

// A non-numeric threshold atom is kept as `None`, which compares as 0.
fn numbers(atoms: &[Atom]) -> Vec<Option<f64>> {
    atoms.iter().take(MAX_SIZE).map(number).collect()
}

fn number(a: &Atom) -> Option<f64> {
    match a.get_value() {
        Some(AtomValue::Int(i)) => Some(i as f64),
        Some(AtomValue::Float(f)) => Some(f),
        _ => None,
    }
}

#[median::methods]
impl Past {
    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.scalar(n as f64);
    }

    #[float]
    pub fn float(&self, f: f64) {
        self.scalar(f);
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        let threshold = self.threshold.lock().unwrap();
        if atoms.len() > threshold.len() {
            return;
        }
        let cmp = |i: usize| compare(number(&atoms[i]), threshold[i]);
        // As in the SDK: the list rises past the threshold when its first element does, or when it
        // ties a run of leading elements and the element after the run is not below.
        let first = cmp(0);
        if first < 0 {
            self.set.store(false, Ordering::Relaxed);
            return;
        }
        let mut canfail = first == 0;
        for i in 1..atoms.len() {
            let result = cmp(i);
            if result < 0 && canfail {
                self.set.store(false, Ordering::Relaxed);
                return;
            }
            canfail = canfail && result == 0;
        }
        self.fire();
    }

    #[sel("clear")]
    pub fn clear(&self) {
        self.set.store(false, Ordering::Relaxed);
    }

    #[gimme("set")]
    pub fn set(&self, atoms: &[Atom]) {
        if !atoms.is_empty() {
            *self.threshold.lock().unwrap() = numbers(atoms);
        }
    }

    fn scalar(&self, v: f64) {
        let threshold = self.threshold.lock().unwrap();
        let Some(&first) = threshold.first() else {
            return;
        };
        let result = compare(Some(v), first);
        if threshold.len() == 1 && result > 0 {
            self.fire();
        } else if threshold.len() == 1 || result < 0 {
            self.set.store(false, Ordering::Relaxed);
        }
    }

    fn fire(&self) {
        if !self.set.swap(true, Ordering::Relaxed) {
            let _ = self.out.send();
        }
    }
}

fn compare(a: Option<f64>, b: Option<f64>) -> i32 {
    match a {
        Some(a) => match a.partial_cmp(&b.unwrap_or(0.)) {
            Some(std::cmp::Ordering::Greater) => 1,
            Some(std::cmp::Ordering::Equal) => 0,
            _ => -1,
        },
        None => -1,
    }
}

median::ext_main! { Past }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn bangs_once_per_rise() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.past", &[10i64.into()]);
        assert!(!x.is_null());
        for v in [5, 11, 12, 3, 20] {
            k.send_int(x, v).unwrap();
        }
        k.send_float(x, 9.5).unwrap();
        k.send_float(x, 10.5).unwrap();
        k.send(x, "clear", &[]).unwrap();
        k.send_float(x, 10.5).unwrap();
        let bangs = k.take_outlet_log();
        assert_eq!(bangs.len(), 4);
        assert!(bangs.iter().all(|e| e.selector == "bang"));
        assert!(k.free_object(x));
    }

    #[test]
    fn list_compares_elementwise() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.past", &[1i64.into(), 5i64.into()]);
        k.send_list(x, &[Atom::Long(1), Atom::Long(4)]).unwrap();
        assert!(k.take_outlet_log().is_empty());
        k.send_list(x, &[Atom::Long(1), Atom::Long(6)]).unwrap();
        assert_eq!(k.take_outlet_log().len(), 1);
        k.send_list(x, &[Atom::Long(0), Atom::Long(9)]).unwrap();
        k.send_list(x, &[Atom::Long(2), Atom::Long(0)]).unwrap();
        assert_eq!(k.take_outlet_log().len(), 1);
        k.send(x, "set", &[Atom::Float(0.5)]).unwrap();
        k.send_float(x, 0.75).unwrap();
        assert!(k.take_outlet_log().is_empty(), "still set from the list");
        k.send_int(x, 0).unwrap();
        k.send_float(x, 0.75).unwrap();
        assert_eq!(k.take_outlet_log().len(), 1);
        assert_eq!(
            k.assist(x, false, 0).as_deref(),
            Some("Input to be Synchronized")
        );
        assert!(k.free_object(x));
    }
}
