use core::ffi::c_void;
use median::{
    jit::{
        JitBuilder, JitObjRef, JitResult, MatrixOp, Ob3d,
        matrix::{Count, IOCount, Matrix},
    },
    prelude::*,
};
use std::sync::atomic::{AtomicUsize, Ordering};

static CALLS: AtomicUsize = AtomicUsize::new(0);
static DROPS: AtomicUsize = AtomicUsize::new(0);

#[derive(MaxObject)]
#[max(name = "jit.test.lifecycle", kind = matrix_op)]
struct Operator {
    obj: JitObjRef,
    #[attr(label = "Gain", clip(0., 4.))]
    gain: Float32,
}
impl MatrixOp for Operator {
    fn new(b: &mut JitBuilder) -> Result<Self, NewError> {
        Ok(Self {
            obj: b.obj(),
            gain: 1.0.into(),
        })
    }
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(0),
            outputs: Count::Fixed(0),
        }
    }
    fn class_setup(
        class: &mut Class<median::jit::wrapper::Core<Self, median::jit::wrapper::MatrixKind>>,
    ) -> median::error::MaxResult<()> {
        class.add_sel(c"core_gain", |s: &Self, v: f64| s.gain.set(v as f32), 0)
    }
    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        assert!(inputs.is_empty());
        assert!(outputs.is_empty());
        CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
impl Drop for Operator {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}
#[median::methods]
impl Operator {
    #[bang]
    fn bang(&self) {
        self.obj.post("bang from the Jitter value");
        CALLS.fetch_add(1, Ordering::SeqCst);
    }
    #[sel("later")]
    #[defer_low]
    fn later(&self) {
        CALLS.fetch_add(10, Ordering::SeqCst);
    }
    #[sel("panic")]
    fn panic(&self) {
        panic!("Jitter message panic");
    }
    #[attr]
    fn broken(&self) -> f64 {
        panic!("Jitter getter panic");
    }
}

#[derive(MaxObject)]
#[max(name = "jit.test.refuse", kind = matrix_op, no_methods)]
struct Refuse;
impl MatrixOp for Refuse {
    fn new(_b: &mut JitBuilder) -> Result<Self, NewError> {
        Err("Jitter creation refused".into())
    }
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(0),
            outputs: Count::Fixed(0),
        }
    }
    fn calc(&self, _: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        Ok(())
    }
}

#[derive(MaxObject)]
#[max(name = "jit.test.gl", kind = gl, no_methods)]
struct Draw;
impl Ob3d for Draw {
    fn new(b: &mut JitBuilder) -> Result<Self, NewError> {
        if b.destination() == sym!("refuse") {
            return Err("GL creation refused".into());
        }
        if b.destination() == sym!("panic") {
            panic!("GL constructor panic");
        }
        Ok(Self)
    }
    fn draw(&self, _: &median::jit::DrawContext) -> JitResult<()> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn dest_closing(&self, _: &median::jit::DrawContext) -> JitResult<()> {
        panic!("GL context callback panic");
    }
}
impl Drop for Draw {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
        panic!("GL Drop panic");
    }
}

median::ext_main! { Operator, Refuse, Draw }

#[derive(MaxObject)]
#[max(name = "jit.test.failedclass", kind = matrix_op, no_methods)]
struct FailedClass {
    #[attr]
    value: Int64,
}
impl MatrixOp for FailedClass {
    fn new(_: &mut JitBuilder) -> Result<Self, NewError> {
        unreachable!("failed class must not be instantiated")
    }
    fn io_count() -> IOCount {
        Refuse::io_count()
    }
    fn class_setup(
        _: &mut Class<median::jit::wrapper::Core<Self, median::jit::wrapper::MatrixKind>>,
    ) -> median::error::MaxResult<()> {
        Err(median::error::MaxError::Generic)
    }
    fn calc(&self, _: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        Ok(())
    }
}

#[test]
fn failed_class_registration_releases_its_attributes_and_can_be_retried() {
    let k = median_mock::lock();
    let attributes = k.live_attribute_count();
    for _ in 0..2 {
        FailedClass::register();
        assert!(!k.class_exists("jit.test.failedclass"));
        assert_eq!(k.live_attribute_count(), attributes);
        assert!(
            k.take_console_log()
                .iter()
                .any(|e| e.text.contains("Jitter class setup failed"))
        );
    }
}

#[test]
fn jitter_registration_callbacks_deferred_ownership_and_partial_construction() {
    let k = median_mock::lock();
    unsafe {
        ext_main(std::ptr::null_mut());
    }
    CALLS.store(0, Ordering::SeqCst);
    DROPS.store(0, Ordering::SeqCst);
    let obj = k.new_object("jit.test.lifecycle", &["@gain".into(), 2.5.into()]);
    assert!(!obj.is_null(), "{:?}", k.console_log());
    assert_eq!(
        k.attr_get(obj, "gain").unwrap(),
        [median_mock::Atom::Float(2.5)]
    );
    k.attr_set(obj, "gain", &[9.0.into()]).unwrap();
    assert_eq!(
        k.attr_get(obj, "gain").unwrap(),
        [median_mock::Atom::Float(4.0)]
    );
    let core = median_mock::jit::object_for(obj);
    assert_ne!(core, obj);
    assert_eq!(k.inlet_is_hot(obj, 0), Some(true));
    assert_eq!(k.inlet_is_hot(obj, 1), Some(false));
    assert!(k.method(obj, "ft1").is_none());
    let (notify, _) = k.method(obj, "notify").unwrap();
    let notify: unsafe extern "C" fn(
        *mut c_void,
        *mut max_sys::t_symbol,
        *mut max_sys::t_symbol,
        *mut c_void,
        *mut c_void,
    ) -> max_sys::t_max_err = unsafe { std::mem::transmute(notify) };
    unsafe {
        notify(
            obj,
            sym!("").as_ptr(),
            sym!("modified").as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
    assert!(
        k.take_console_log()
            .iter()
            .any(|e| e.text.contains("SDK Jitter notification delivered"))
    );
    k.send(core, "core_gain", &[1.5.into()]).unwrap();
    assert_eq!(
        k.attr_get(obj, "gain").unwrap(),
        [median_mock::Atom::Float(1.5)]
    );
    let (calc, _) = k.method(core, "matrix_calc").unwrap();
    let calc: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> max_sys::t_jit_err =
        unsafe { std::mem::transmute(calc) };
    assert_eq!(
        unsafe { calc(core, std::ptr::null_mut(), std::ptr::null_mut()) },
        0
    );
    {
        use median::jit::matrix::{MatrixDataType, MatrixInfo};
        use median_mock::jit::{MatrixFixture, MatrixList};
        let fixture = MatrixFixture::new(
            *MatrixInfo::new(MatrixDataType::Char, 1, &[1])
                .unwrap()
                .as_raw(),
        );
        let too_many = MatrixList::new(&[&fixture; 33]);
        assert_eq!(
            unsafe { calc(core, too_many.as_ptr(), std::ptr::null_mut()) },
            max_sys::t_jit_error_code::JIT_ERR_OUT_OF_BOUNDS as _
        );
    }
    k.send_bang(obj).unwrap();
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    k.send(obj, "later", &[]).unwrap();
    k.run_main_queue();
    assert_eq!(CALLS.load(Ordering::SeqCst), 12);
    k.send(obj, "panic", &[]).unwrap();
    assert!(
        k.take_console_log()
            .iter()
            .any(|e| e.text.contains("Jitter message panic"))
    );
    assert!(k.attr_get(obj, "broken").is_err());
    assert!(
        k.take_console_log()
            .iter()
            .any(|e| e.text.contains("Jitter getter panic"))
    );
    assert!(k.assist(obj, false, 0).unwrap().contains("Jitter matrix"));
    k.send(obj, "later", &[]).unwrap();
    assert!(k.free_object(obj));
    k.run_main_queue();
    assert_eq!(CALLS.load(Ordering::SeqCst), 12);
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(k.live_object_count(), 0);
    assert!(k.new_object("jit.test.refuse", &[]).is_null());
    assert_eq!(k.live_object_count(), 0);
    assert!(k.new_object("jit.test.gl", &["refuse".into()]).is_null());
    assert!(k.new_object("jit.test.gl", &["panic".into()]).is_null());
    assert_eq!(k.live_object_count(), 0);
    assert_eq!(median_mock::jit::context_count(), 0);
    let gl = k.new_object("jit.test.gl", &["context".into()]);
    assert!(!gl.is_null());
    let core = median_mock::jit::object_for(gl);
    assert_eq!(median_mock::jit::context_count(), 1);
    for (selector, ok) in [("ob3d_draw", true), ("dest_closing", false)] {
        let (f, _) = k.method(core, selector).unwrap();
        let f: unsafe extern "C" fn(*mut c_void) -> max_sys::t_jit_err =
            unsafe { std::mem::transmute(f) };
        assert_eq!(unsafe { f(core) } == 0, ok);
    }
    assert!(k.method(core, "register").is_some());
    assert!(k.free_object(gl));
    assert_eq!(median_mock::jit::context_count(), 0);
    assert_eq!(k.live_object_count(), 0);
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);
}
