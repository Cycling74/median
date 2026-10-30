//! A port of the SDK's `patcher/howbigisyourp`: on bang, counts the objects of each class in its
//! patcher and the subpatchers; `dump` posts the total and outputs `<class> <count>` for each
//! class, fewest first.
//!
//! Classes with the same count come out in name order (the C's sort leaves them unordered). A box
//! whose object failed to be made is skipped (the C asks a null object for its class name).
//! Finding the patcher and iterating it have no median API; median-mock has no patcher, so the
//! tests cover counting and `dump` without one.

use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::{c_long, c_void},
    ptr::null_mut,
    sync::Mutex,
};

use median::{max_sys, prelude::*};

// GAP: PI_DEEP and PI_WANTBOX (`ext_maxtypes.h`) are not in max_sys.
const PI_DEEP: c_long = 1;
const PI_WANTBOX: c_long = 4;

type Counts = HashMap<SymbolRef, i64>;

#[derive(MaxObj)]
#[max(name = "median.howbigisyourp")]
pub struct HowBigIsYourP {
    obj: ObjRef,
    counts: Mutex<Counts>,
    out: Outlet<Any>,
}

impl MaxObjWrapped for HowBigIsYourP {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            obj: builder.obj(),
            counts: Mutex::new(HashMap::new()),
            out: builder.outlet(Outlet::any()),
        })
    }

    fn assist(&self, iolet: IoLet) -> Option<Cow<'_, str>> {
        Some(
            match iolet {
                IoLet::Inlet(_) => "bang, dump",
                IoLet::Outlet(_) => "(list) {Classname instances}",
            }
            .into(),
        )
    }
}

#[median::methods]
impl HowBigIsYourP {
    #[bang]
    pub fn bang(&self) {
        let mut counts = self.counts.lock().unwrap();
        counts.clear();
        let mut jp = null_mut();
        // SAFETY: the object is live and `jp` is writable; it stays null if there is no patcher.
        // GAP: object_obex_lookup
        unsafe {
            max_sys::object_obex_lookup(
                self.obj.as_ptr().cast(),
                median::sym!("#P").as_ptr(),
                &mut jp,
            )
        };
        if jp.is_null() {
            return;
        }
        let mut result: c_long = 0;
        // SAFETY: the patcher outlives the objects in it; `iterate` calls `count_box` with the
        // counts and each box before it returns.
        // GAP: object_method
        unsafe {
            max_sys::object_method(
                jp.cast(),
                median::sym!("iterate").as_ptr(),
                count_box as unsafe extern "C" fn(*mut c_void, *mut max_sys::t_object) -> c_long,
                &mut *counts as *mut Counts,
                PI_DEEP | PI_WANTBOX,
                &mut result as *mut c_long,
            )
        };
    }

    #[sel("dump")]
    pub fn dump(&self) {
        let (total, sorted) = report(&self.counts.lock().unwrap());
        post!("The total number of objects is {total}");
        for (name, n) in sorted {
            let _ = self.out.send(&name, &[Atom::from(n)]);
        }
    }
}

/// Counts the class of the object in box `b` into `counts`.
///
/// # Safety
/// `counts` must point to a `Counts` not otherwise borrowed, and `b` to a live box.
unsafe extern "C" fn count_box(counts: *mut c_void, b: *mut max_sys::t_object) -> c_long {
    // SAFETY: the caller's contract; Max's class names are interned.
    // GAP: patcher access (jbox_get_object), object_classname
    unsafe {
        let mut name = SymbolRef::from_raw(max_sys::object_classname(b.cast()));
        // A `newobj` box holds an object of another class (e.g. `borax`).
        if name == median::sym!("newobj") {
            let o = max_sys::jbox_get_object(b);
            if o.is_null() {
                return 0;
            }
            name = SymbolRef::from_raw(max_sys::object_classname(o.cast()));
        }
        tally(&mut *(counts as *mut Counts), name);
    }
    0
}

fn tally(counts: &mut Counts, name: SymbolRef) {
    if !name.is_empty() {
        *counts.entry(name).or_default() += 1;
    }
}

/// The total count, and every class with its count, fewest first.
fn report(counts: &Counts) -> (i64, Vec<(SymbolRef, i64)>) {
    let mut sorted: Vec<_> = counts.iter().map(|(&s, &n)| (s, n)).collect();
    sorted.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| a.0.to_string().cmp(&b.0.to_string()))
    });
    (counts.values().sum(), sorted)
}

median::ext_main! { HowBigIsYourP }

#[cfg(test)]
mod tests {
    use super::*;
    use median_mock::Atom as MockAtom;

    #[test]
    fn outside_a_patcher_counts_nothing() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.howbigisyourp", &[]);
        assert!(!x.is_null());
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("bang, dump"));
        assert_eq!(
            k.assist(x, true, 0).as_deref(),
            Some("(list) {Classname instances}")
        );
        k.send_bang(x).unwrap();
        k.send(x, "dump", &[]).unwrap();
        assert!(k.take_outlet_log().is_empty());
        let console: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(console, ["The total number of objects is 0"]);
        assert!(k.free_object(x));
    }

    #[test]
    fn dump_outputs_counts_fewest_first() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.howbigisyourp", &[]);
        let names = [
            "toggle", "borax", "message", "toggle", "message", "toggle", "",
        ];
        {
            // SAFETY: `x` is a live `median.howbigisyourp`.
            let w = unsafe { &*(x as *const MaxObjWrapper<HowBigIsYourP>) };
            let mut counts = w.wrapped().counts.lock().unwrap();
            for n in names {
                tally(&mut counts, SymbolRef::try_from(n).unwrap());
            }
        }
        k.send(x, "dump", &[]).unwrap();
        let out: Vec<_> = k
            .take_outlet_log()
            .into_iter()
            .map(|e| (e.selector, e.atoms))
            .collect();
        assert_eq!(
            out,
            [
                ("borax".into(), vec![MockAtom::Long(1)]),
                ("message".into(), vec![MockAtom::Long(2)]),
                ("toggle".into(), vec![MockAtom::Long(3)]),
            ]
        );
        assert_eq!(
            k.take_console_log()[0].text,
            "The total number of objects is 6"
        );
        assert!(k.free_object(x));
    }
}
