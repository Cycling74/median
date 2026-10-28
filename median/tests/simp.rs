//! The `simp` example (`#[derive(MaxObj)]`, `#[median::methods]`, `ext_main!`), compiled into
//! this test and run against the `median-mock` kernel.

mod common;
#[path = "../examples/simp/src/lib.rs"]
mod simp;

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
    assert!(
        console[0].text.contains("doesn't understand"),
        "{console:?}"
    );
    assert!(console[0].text.contains("float"), "{console:?}");
    assert!(k.take_outlet_log().is_empty());
    assert!(k.free_object(x));
}

#[test]
fn simp_do_abs_answers_a_typed_caller() {
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { simp::ext_main(std::ptr::null_mut()) };
    let t = common::simp_script::run_do_abs(&k, "simp");
    assert_eq!(t, common::simp_script::EXPECTED_DO_ABS, "{t:#?}");
    assert_eq!(median_mock::containers::live_count(), 0);
}

#[test]
fn simp_registers_without_errors_and_has_a_notify_method() {
    // `simp` has a `#[notify]` method: `Class::add_notify` must not see one that is not there
    // (Max 9.2 printed `failed to register simp: ... Duplicate` for it on 2026-10-27)
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { simp::ext_main(std::ptr::null_mut()) };
    assert!(k.class_exists("simp"), "{:?}", k.console_log());
    assert!(
        !k.console_log()
            .iter()
            .any(|e| e.text.contains("failed to register")),
        "{:?}",
        k.console_log()
    );
    let x = k.new_object("simp", &[]);
    assert!(!x.is_null());
    assert!(k.method(x, "notify").is_some());
    assert!(k.free_object(x));
}
