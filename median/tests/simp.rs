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
