//! The `simp` example (`#[derive(MaxObject)]`, `#[median::methods]`, `ext_main!`), compiled into
//! this test and run against the `median-mock` kernel.

#[path = "../examples/simp/src/lib.rs"]
mod simp;
mod common;

#[test]
fn simp_runs_the_script() {
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { simp::ext_main(std::ptr::null_mut()) };
    assert!(k.class_exists("simp"));
    let t = common::simp_script::run(&k, "simp");
    assert_eq!(t, common::simp_script::EXPECTED, "{t:#?}");
}

#[test]
fn simp_takes_or_refuses_its_argument() {
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { simp::ext_main(std::ptr::null_mut()) };
    let t = common::simp_script::run_args(&k, "simp");
    assert_eq!(t, common::simp_script::EXPECTED_ARGS, "{t:#?}");
}

#[test]
fn simp_does_not_understand_float_in_its_left_inlet() {
    // checked in Max: simp has `int`, `list` and `anything` methods but no `float`, and Max posts
    // `simp: doesn't understand "float"` instead of truncating or falling back
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { simp::ext_main(std::ptr::null_mut()) };
    let x = k.new_object("simp", &[]);
    assert!(!x.is_null());
    k.take_console_log();
    assert!(matches!(
        k.send_float(x, 1.5),
        Err(median_mock::SendError::NoMethod(_))
    ));
    let console = k.take_console_log();
    assert_eq!(console.len(), 1, "{console:?}");
    assert!(console[0].text.contains("doesn't understand"), "{console:?}");
    assert!(console[0].text.contains("float"), "{console:?}");
    assert!(k.take_outlet_log().is_empty());
    assert!(k.free_object(x));
}
