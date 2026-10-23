//! Run the SDK example port through the real registration and construction trampolines.

#[path = "../examples/simplemax/src/lib.rs"]
mod simplemax;

use median_mock::{Atom, Level};

#[test]
fn simplemax_reports_arguments_and_inlet_help() {
    let k = median_mock::lock();
    // SAFETY: registration on the mock's main thread, as Max calls it.
    unsafe { simplemax::ext_main(std::ptr::null_mut()) };
    assert!(k.class_exists("simplemax"));
    let registration = k.take_console_log();
    assert_eq!(registration.len(), 1);
    assert_eq!(registration[0].text, "I am the simplemax object");
    assert_eq!(registration[0].object, 0);

    for (args, expected) in [
        (vec![], vec!["it has 0 arguments"]),
        (
            vec![42i64.into(), 1.25f64.into(), "hello%s".into()],
            vec![
                "it has 3 arguments",
                "arg 0: long (42)",
                "arg 1: float (1.250000)",
                "arg 2: symbol (hello%s)",
            ],
        ),
    ] {
        let x = k.new_object("simplemax", &args);
        assert!(!x.is_null());
        let log = k.take_console_log();
        assert!(
            log.iter()
                .all(|e| e.level == Level::Post && e.object == x as usize)
        );
        assert_eq!(log[0].text, "a new simplemax object was instantiated");
        assert_eq!(
            log[1..].iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("I am inlet 0"));
        assert!(k.take_outlet_log().is_empty());
        assert!(k.free_object(x));
        assert_eq!(k.live_object_count(), 0);
    }
    // As in the SDK, an unsupported argument is reported but does not refuse creation.
    let x = k.new_object("simplemax", &[Atom::Other(0)]);
    assert!(!x.is_null());
    let log = k.take_console_log();
    assert_eq!(log.last().unwrap().level, Level::Error);
    assert_eq!(log.last().unwrap().object, x as usize);
    assert_eq!(log.last().unwrap().text, "forbidden argument");
    assert!(k.free_object(x));
}
