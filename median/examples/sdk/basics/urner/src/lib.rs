//! A port of the SDK's `basics/urner`: bang draws a random number below the size argument
//! without repeats; once all are drawn, bang goes out the right outlet until `reset`.
//!
//! The SDK uses the C library's `random()`; this uses a xorshift generator seeded from the clock.
//! A missing or non-positive size refuses the object with an error, where the SDK fails silently.

use std::{
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use median::prelude::*;

struct Urn {
    drawn: Vec<bool>,
    count: usize,
    rng: XorShift,
}

#[derive(MaxObj)]
#[max(name = "median.urner")]
pub struct Urner {
    urn: Mutex<Urn>,
    out: Outlet<Int>,
    empty: Outlet<Bang>,
}

impl MaxObjWrapped for Urner {
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        post!("I am the urner object");
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let size = match builder.args().at(0).get::<t_atom_long>()? {
            Some(n) if n >= 1 => n as usize,
            _ => return Err("urner needs a size of at least 1".into()),
        };
        builder.inlet(Inlet::default().assist("I am inlet 0"));
        Ok(Self {
            urn: Mutex::new(Urn {
                drawn: vec![false; size],
                count: 0,
                rng: XorShift::from_time(),
            }),
            out: builder.outlet(Outlet::int().assist("I am outlet 0")),
            empty: builder.outlet(Outlet::bang().assist("I am outlet 1")),
        })
    }
}

#[median::methods]
impl Urner {
    #[bang]
    pub fn bang(&self) {
        let mut urn = self.urn.lock().unwrap();
        let size = urn.drawn.len();
        if urn.count == size {
            drop(urn);
            let _ = self.empty.send();
            return;
        }
        let n = loop {
            let n = (urn.rng.next() % size as u64) as usize;
            if !urn.drawn[n] {
                break n;
            }
        };
        urn.drawn[n] = true;
        urn.count += 1;
        drop(urn);
        let _ = self.out.send(n as t_atom_long);
    }

    #[sel("reset")]
    pub fn reset(&self) {
        let mut urn = self.urn.lock().unwrap();
        urn.drawn.fill(false);
        urn.count = 0;
    }
}

struct XorShift(u64);

impl XorShift {
    fn from_time() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        // xorshift never leaves 0.
        Self(nanos | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

median::ext_main! { Urner }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn draw(k: &median_mock::Kernel, x: *mut std::ffi::c_void, n: usize) -> Vec<i64> {
        for _ in 0..n {
            k.send_bang(x).unwrap();
        }
        k.take_outlet_log()
            .into_iter()
            .map(|e| match (e.index, &e.atoms[..]) {
                (0, [Atom::Long(v)]) => *v,
                (1, []) => -1,
                _ => panic!("unexpected output {e:?}"),
            })
            .collect()
    }

    #[test]
    fn draws_each_number_once_until_reset() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.urner", &[Atom::Long(5)]);
        assert!(!x.is_null());
        let mut first = draw(&k, x, 7);
        assert_eq!(&first[5..], [-1, -1], "bang on the right outlet when empty");
        first.truncate(5);
        first.sort();
        assert_eq!(first, [0, 1, 2, 3, 4]);
        k.send(x, "reset", &[]).unwrap();
        let mut again = draw(&k, x, 5);
        again.sort();
        assert_eq!(again, [0, 1, 2, 3, 4]);
        assert_eq!(k.assist(x, true, 1).as_deref(), Some("I am outlet 1"));
        assert!(k.free_object(x));
    }

    #[test]
    fn refuses_a_bad_size() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        assert!(k.new_object("median.urner", &[]).is_null());
        assert!(k.new_object("median.urner", &[Atom::Long(0)]).is_null());
    }
}
