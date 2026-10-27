//! The `simp_explicit` example (no macros but `ext_main!`), compiled into this test and run
//! against the `median-mock` kernel: it behaves exactly like `simp`.

mod common;
#[path = "../examples/simp_explicit/src/lib.rs"]
mod simp_explicit;

#[test]
fn simp_explicit_behaves_like_simp() {
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { simp_explicit::ext_main(std::ptr::null_mut()) };
    assert!(k.class_exists("simp_explicit"));
    let t = common::simp_script::run(&k, "simp_explicit");
    assert_eq!(t, common::simp_script::EXPECTED, "{t:#?}");
}

#[test]
fn simp_explicit_takes_or_refuses_its_argument() {
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { simp_explicit::ext_main(std::ptr::null_mut()) };
    let t = common::simp_script::run_args(&k, "simp_explicit");
    assert_eq!(t, common::simp_script::EXPECTED_ARGS, "{t:#?}");
}

#[test]
fn simp_explicit_do_abs_answers_a_typed_caller() {
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { simp_explicit::ext_main(std::ptr::null_mut()) };
    let t = common::simp_script::run_do_abs(&k, "simp_explicit");
    assert_eq!(t, common::simp_script::EXPECTED_DO_ABS, "{t:#?}");
    assert_eq!(median_mock::containers::live_count(), 0);
}
