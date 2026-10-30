//! A port of the SDK's `advanced/dllexample/dllcounter`: bang increments the counter in the
//! shared library `mydll`, which every object linked against it shares.

use median::prelude::*;

unsafe extern "C" {
    fn mydll_init();
    fn mydll_inc();
}

#[derive(MaxObj)]
#[max(name = "median.dllcounter")]
pub struct Dllcounter {
    _obj: ObjRef,
}

impl MaxObjWrapped for Dllcounter {
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        // SAFETY: mydll's functions take no arguments and may be called from any thread.
        unsafe { mydll_init() };
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("bang to increase the count by 1"));
        Ok(Self {
            _obj: builder.obj(),
        })
    }
}

#[median::methods]
impl Dllcounter {
    #[bang]
    pub fn bang(&self) {
        // SAFETY: as in `class_setup`.
        unsafe { mydll_inc() };
    }
}

median::ext_main! { Dllcounter }

#[cfg(test)]
mod tests {
    unsafe extern "C" {
        fn mydll_getcount() -> std::ffi::c_int;
    }

    #[test]
    fn bang_increments_the_shared_count() {
        // Keeps the mock's `post` in the test binary, where libmydll looks it up.
        std::hint::black_box(median::max_sys::post as *const ());
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let posts: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(posts, ["initialized mydll"]);
        let a = k.new_object("median.dllcounter", &[]);
        let b = k.new_object("median.dllcounter", &[]);
        assert!(!a.is_null() && !b.is_null());
        // SAFETY: takes no arguments.
        let before = unsafe { mydll_getcount() };
        k.send_bang(a).unwrap();
        k.send_bang(b).unwrap();
        k.send_bang(a).unwrap();
        // SAFETY: as above.
        assert_eq!(unsafe { mydll_getcount() }, before + 3);
        assert_eq!(
            k.assist(a, false, 0).as_deref(),
            Some("bang to increase the count by 1")
        );
        assert_eq!(k.outlet_count(a), 0);
        assert!(k.take_outlet_log().is_empty());
        assert!(k.free_object(a));
        assert!(k.free_object(b));
    }
}
