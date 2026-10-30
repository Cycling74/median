//! A port of the SDK's `advanced/dllexample/mydll`: a shared library holding one counter that
//! the `median.dllcounter` and `median.dllreport` externals link against.
//!
//! It is not an external: it builds `libmydll.dylib`, which belongs in the package's `support`
//! folder. `post` is resolved when Max loads it.

use std::{
    ffi::c_int,
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};

use median::max_sys;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static COUNT: AtomicI32 = AtomicI32::new(0);

/// Posts to the Max console the first time any external calls it.
#[unsafe(no_mangle)]
pub extern "C" fn mydll_init() {
    if !INITIALIZED.swap(true, Ordering::Relaxed) {
        // SAFETY: a NUL-terminated format string without format codes.
        unsafe { max_sys::post(c"initialized mydll".as_ptr()) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mydll_inc() {
    COUNT.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "C" fn mydll_getcount() -> c_int {
    COUNT.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts() {
        let before = super::mydll_getcount();
        super::mydll_inc();
        super::mydll_inc();
        assert_eq!(super::mydll_getcount(), before + 2);
    }
}
