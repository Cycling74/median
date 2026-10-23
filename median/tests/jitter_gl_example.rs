#[path = "../examples/jit_gl_simple/src/lib.rs"]
mod gl_simple;
use core::ffi::c_void;

#[test]
fn gl_example_records_the_sdk_quad() {
    let k = median_mock::lock();
    unsafe {
        gl_simple::ext_main(std::ptr::null_mut());
    }
    let object = k.new_object("jit.gl.median.simple", &["render-context".into()]);
    assert!(!object.is_null());
    let core = median_mock::jit::object_for(object);
    let (f, _) = k.method(core, "ob3d_draw").unwrap();
    let draw: unsafe extern "C" fn(*mut c_void) -> max_sys::t_jit_err =
        unsafe { std::mem::transmute(f) };
    assert_eq!(unsafe { draw(core) }, 0);
    use median_mock::jit::GlCall::*;
    assert_eq!(
        median_mock::jit::take_gl_log(),
        [
            Begin(max_sys::e_jit_state::JIT_STATE_QUADS),
            Vertex([-1., -1., 0.]),
            Vertex([-1., 1., 0.]),
            Vertex([1., 1., 0.]),
            Vertex([1., -1., 0.]),
            End,
        ]
    );
    assert!(k.free_object(object));
    assert_eq!(median_mock::jit::context_count(), 0);
}
