//! A port of the SDK's `advanced/dllexample/dllreport`: bang outputs the counter in the shared
//! library `mydll`, which every object linked against it shares.

use median::prelude::*;

unsafe extern "C" {
    fn mydll_init();
    fn mydll_getcount() -> std::ffi::c_int;
}

#[derive(MaxObj)]
#[max(name = "median.dllreport")]
pub struct Dllreport {
    _obj: ObjRef,
    out: Outlet<Int>,
}

impl MaxObjWrapped for Dllreport {
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        // SAFETY: mydll's functions take no arguments and may be called from any thread.
        unsafe { mydll_init() };
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("bang to get the current count"));
        Ok(Self {
            _obj: builder.obj(),
            out: builder.outlet(Outlet::int().assist("(int) the count")),
        })
    }
}

#[median::methods]
impl Dllreport {
    #[bang]
    pub fn bang(&self) {
        // SAFETY: as in `class_setup`.
        let count = unsafe { mydll_getcount() };
        let _ = self.out.send(count as t_atom_long);
    }
}

median::ext_main! { Dllreport }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    unsafe extern "C" {
        fn mydll_inc();
    }

    #[test]
    fn bang_outputs_the_shared_count() {
        // Keeps the mock's `post` in the test binary, where libmydll looks it up.
        std::hint::black_box(median::max_sys::post as *const ());
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let posts: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(posts, ["initialized mydll"]);
        let x = k.new_object("median.dllreport", &[]);
        assert!(!x.is_null());
        k.send_bang(x).unwrap();
        // SAFETY: takes no arguments.
        unsafe { mydll_inc() };
        k.send_bang(x).unwrap();
        let counts: Vec<_> = k.take_outlet_log().into_iter().map(|e| e.atoms).collect();
        assert_eq!(counts, [vec![Atom::Long(0)], vec![Atom::Long(1)]]);
        assert_eq!(
            k.assist(x, false, 0).as_deref(),
            Some("bang to get the current count")
        );
        assert_eq!(k.assist(x, true, 0).as_deref(), Some("(int) the count"));
        assert!(k.free_object(x));
    }
}
