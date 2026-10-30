//! A port of the SDK's `misc/thresh`: gathers numbers that arrive within a time threshold of each
//! other into a list.
//!
//! The SDK reads the object's scheduler time with `gettime_forobject`; this port reads the
//! current scheduler time (`clock_getftime`), which is the same unless the object's scheduler is
//! not the one running.

use std::sync::Mutex;

use median::{atom::AtomValue, prelude::*};

const MAX_SIZE: usize = 4096;
const MIN_INTERVAL: f64 = 5.;

// `Atom` is not `Send`, so the gathered list holds these instead.
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

#[derive(MaxObj)]
#[max(name = "median.thresh")]
pub struct Thresh {
    obj: ObjRef,
    list: Mutex<Vec<Value>>,
    interval: Float64,
    // When the last element arrived, in scheduler milliseconds.
    time: Float64,
    clock: Clock,
    out: Outlet<Any>,
}

impl MaxObjWrapped for Thresh {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let interval: f64 = builder.args().at(0).unwrap_or(0.)?;
        builder.inlet(Inlet::default().assist("Numbers to be Gathered Into list"));
        builder.inlet(Inlet::proxy().assist("Arrival Threshold"));
        Ok(Self {
            obj: builder.obj(),
            list: Mutex::new(Vec::new()),
            interval: Float64::new(interval.max(MIN_INTERVAL)),
            time: Float64::new(Clock::ftime()),
            clock: builder.clock(Self::tick),
            out: builder.outlet(Outlet::any().assist("Gathered list")),
        })
    }
}

#[median::methods]
impl Thresh {
    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.number(Value::Int(n), n as f64);
    }

    #[float]
    pub fn float(&self, f: f64) {
        self.number(Value::Float(f), f);
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        self.time.set(Clock::ftime());
        let mut list = self.list.lock().unwrap();
        let room = MAX_SIZE - list.len();
        list.extend(
            atoms
                .iter()
                .filter_map(|a| match a.get_value()? {
                    AtomValue::Int(i) => Some(Value::Int(i)),
                    AtomValue::Float(f) => Some(Value::Float(f)),
                    AtomValue::Symbol(s) => Some(Value::Sym(s)),
                    AtomValue::Object(_) => None,
                })
                .take(room),
        );
        self.clock.fdelay(self.interval.get());
    }

    fn number(&self, a: Value, v: f64) {
        if self.obj.proxy_inlet() != 0 {
            self.interval.set(v.max(MIN_INTERVAL));
            return;
        }
        let mut list = self.list.lock().unwrap();
        // As in the SDK, a single number is refused one element earlier than a list.
        if list.len() < MAX_SIZE - 1 {
            self.time.set(Clock::ftime());
            list.push(a);
            self.clock.fdelay(self.interval.get());
        }
    }

    fn tick(&self) {
        let interval = self.interval.get();
        if Clock::ftime() - self.time.get() < interval {
            self.clock.fdelay(interval);
            return;
        }
        let list = std::mem::take(&mut *self.list.lock().unwrap());
        if !list.is_empty() {
            let list: Vec<Atom> = list.into_iter().map(Atom::from).collect();
            let _ = self.out.send_list(&list);
        }
    }
}

median::ext_main! { Thresh }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn gathers_numbers_within_the_threshold() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.thresh", &[Atom::Float(20.)]);
        assert!(!x.is_null());
        k.send_int(x, 1).unwrap();
        k.advance(10.);
        k.send_float(x, 2.5).unwrap();
        k.advance(10.);
        k.send_list(x, &[Atom::Long(3), Atom::Long(4)]).unwrap();
        k.advance(19.);
        assert!(k.take_outlet_log().is_empty());
        k.advance(1.);
        let out: Vec<_> = k
            .take_outlet_log()
            .into_iter()
            .map(|e| (e.selector, e.atoms))
            .collect();
        assert_eq!(
            out,
            [(
                "list".to_string(),
                vec![
                    Atom::Long(1),
                    Atom::Float(2.5),
                    Atom::Long(3),
                    Atom::Long(4)
                ]
            )]
        );
        k.advance(100.);
        assert!(k.take_outlet_log().is_empty());
        assert!(k.free_object(x));
    }

    #[test]
    fn right_inlet_sets_the_threshold() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.thresh", &[]);
        k.send_inlet(x, 1, "int", &[Atom::Long(50)]).unwrap();
        k.send_int(x, 1).unwrap();
        k.advance(49.);
        assert!(k.take_outlet_log().is_empty());
        k.advance(1.);
        assert_eq!(k.take_outlet_log().len(), 1);
        // Below the 5 ms minimum.
        k.send_inlet(x, 1, "float", &[Atom::Float(1.)]).unwrap();
        k.send_int(x, 2).unwrap();
        k.advance(4.);
        assert!(k.take_outlet_log().is_empty());
        k.advance(1.);
        assert_eq!(k.take_outlet_log()[0].atoms, [Atom::Long(2)]);
        assert_eq!(k.assist(x, false, 1).as_deref(), Some("Arrival Threshold"));
        assert!(k.free_object(x));
    }
}
