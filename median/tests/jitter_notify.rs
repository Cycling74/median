//! The `notify` method of a Jitter class when the SDK's wrapper installs none: median's glue
//! installs its own, which reaches `JitterObject::notify` (for a matrix operator, the SDK's
//! `max_jit_mop_notify`). `jitter.rs` covers the class whose wrapper has the SDK's own.

use core::ffi::c_void;
use median::{
    jit::{
        JitBuilder, JitObjRef, JitResult, MatrixOp,
        matrix::{Count, IOCount, Matrix},
    },
    prelude::*,
};
use median_mock::Atom as MockAtom;

#[derive(MaxObject)]
#[max(name = "jit.test.nonotify", kind = matrix_op, no_methods)]
struct NoSdkNotify {
    _obj: JitObjRef,
}
impl MatrixOp for NoSdkNotify {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { _obj: b.obj() })
    }
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }
    fn calc(&self, _: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        Ok(())
    }
}

median::ext_main! { NoSdkNotify }

type NotifyFn = unsafe extern "C" fn(
    *mut c_void,
    *mut max_sys::t_symbol,
    *mut max_sys::t_symbol,
    *mut c_void,
    *mut c_void,
);

#[test]
fn a_notification_reaches_the_jitter_object_when_the_sdk_wrapper_has_no_notify() {
    let k = median_mock::lock();
    median_mock::jit::skip_sdk_notify();
    // SAFETY: called as Max does, once per kernel
    unsafe { ext_main(std::ptr::null_mut()) };
    assert!(k.class_exists("jit.test.nonotify"), "{:?}", k.console_log());
    let obj = k.new_object("jit.test.nonotify", &[] as &[MockAtom]);
    assert!(!obj.is_null(), "{:?}", k.console_log());
    // median's own `notify` is there now
    let (notify, _) = k.method(obj, "notify").expect("a notify method");
    // SAFETY: `notify` is the `A_CANT` method `(x, s, msg, sender, data)`
    let notify: NotifyFn = unsafe { std::mem::transmute(notify) };
    k.take_console_log();
    median_mock::jit::take_mop_notify_calls();
    unsafe {
        notify(
            obj,
            sym!("somebody").as_ptr(),
            sym!("modified").as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
    // the SDK's wrapper did not install one, so it was median's `forward_notify` that called
    // `max_jit_mop_notify` with the object, the sender's name and the message
    assert_eq!(
        median_mock::jit::take_mop_notify_calls(),
        [(obj as usize, "somebody".to_string(), "modified".to_string())]
    );
    assert!(
        !k.take_console_log()
            .iter()
            .any(|e| e.text.contains("SDK Jitter notification delivered"))
    );
    assert!(k.free_object(obj));
}
