#[path = "../examples/jit_scalebias/src/lib.rs"]
mod scalebias;
use core::ffi::c_void;
use median::jit::matrix::{MatrixDataType, MatrixInfo};
use median_mock::jit::{MatrixFixture, MatrixList};

#[test]
fn scalebias_transforms_argb_and_supports_in_place_and_intersections() {
    let k = median_mock::lock();
    unsafe {
        scalebias::ext_main(std::ptr::null_mut());
    }
    let object = k.new_object("jit.median.scalebias", &["@rscale".into(), 2.0.into()]);
    assert!(!object.is_null(), "{:?}", k.console_log());
    let core = median_mock::jit::object_for(object);
    let (f, _) = k.method(core, "matrix_calc").unwrap();
    let calc: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> max_sys::t_jit_err =
        unsafe { std::mem::transmute(f) };
    let info = MatrixInfo::new(MatrixDataType::Char, 4, &[2, 2]).unwrap();
    let mut input = MatrixFixture::new(*info.as_raw());
    input.write_bytes(&[
        255, 100, 20, 30, 255, 200, 50, 60, 255, 70, 80, 90, 255, 1, 2, 3,
    ]);
    let output = MatrixFixture::new(*info.as_raw());
    {
        let ins = MatrixList::new(&[&input]);
        let outs = MatrixList::new(&[&output]);
        assert_eq!(unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }, 0);
    }
    assert_eq!(
        output.bytes(),
        [
            255, 200, 20, 30, 255, 255, 50, 60, 255, 140, 80, 90, 255, 2, 2, 3
        ]
    );
    assert_eq!(input.lock_state(), 0);
    assert_eq!(output.lock_state(), 0);
    k.send(object, "scale", &[0.5.into()]).unwrap();
    {
        let in_place = MatrixList::new(&[&input]);
        assert_eq!(
            unsafe { calc(core, in_place.as_ptr(), in_place.as_ptr()) },
            0
        );
    }
    assert_eq!(
        input.bytes(),
        [
            127, 50, 10, 15, 127, 100, 25, 30, 127, 35, 40, 45, 127, 0, 1, 1
        ]
    );
    k.send(object, "scale", &[1.0.into()]).unwrap();
    let mut cropped = MatrixFixture::new(
        *MatrixInfo::new(MatrixDataType::Char, 4, &[1, 3])
            .unwrap()
            .as_raw(),
    );
    cropped.write_bytes(&[9; 12]);
    {
        let ins = MatrixList::new(&[&input]);
        let outs = MatrixList::new(&[&cropped]);
        assert_eq!(unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }, 0);
    }
    assert_eq!(
        cropped.bytes(),
        [127, 50, 10, 15, 127, 35, 40, 45, 9, 9, 9, 9]
    );
    assert!(k.free_object(object));
    assert_eq!(k.live_object_count(), 0);
}
