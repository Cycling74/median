//! Exercise the exact diagnostic external shipped with the Max validation patches.
#[path = "../examples/validation/src/lib.rs"]
mod validation;

use median_mock::{Atom, Kernel, Role};

fn register() {
    // SAFETY: called as Max calls ext_main, while holding the mock kernel lock.
    unsafe { validation::ext_main(std::ptr::null_mut()) };
}

fn posts(k: &Kernel) -> Vec<String> {
    k.take_console_log().into_iter().map(|e| e.text).collect()
}

#[test]
fn messages_report_actual_threads_and_pending_calls_are_cancelled() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("median.validate", &["fixture".into()]);
    assert!(!obj.is_null());
    assert!(k.loadbang(obj));
    assert_eq!(
        posts(&k),
        ["fixture: loadbang; main=true scheduler=false audio=false"]
    );
    assert!(k.method(obj, "int").is_none());
    // Observed in Max on 2026-10-26: a float method alone does not accept int messages.
    assert_eq!(
        k.send_int(obj, 17),
        Err(median_mock::SendError::NoMethod("int".into()))
    );
    let errors = posts(&k);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("doesn't understand") && errors[0].contains("int"));
    k.send_float(obj, 17.5).unwrap();
    assert_eq!(
        posts(&k),
        ["fixture: float 17.5; main=true scheduler=false audio=false"]
    );
    let args = [Atom::Long(42), Atom::Sym("hello".into())];
    k.with_role(Role::Timer, || {
        k.send(obj, "direct", &args).unwrap();
        k.send(obj, "low", &args).unwrap();
        k.send(obj, "front", &args).unwrap();
    });
    let immediate = posts(&k);
    assert_eq!(immediate.len(), 1);
    assert!(immediate[0].contains("direct"));
    assert!(immediate[0].contains("main=false scheduler=true audio=false"));
    k.run_main_queue();
    let deferred = posts(&k);
    assert_eq!(deferred.len(), 2);
    for (line, selector) in deferred.iter().zip(["front", "low"]) {
        assert!(line.contains(selector) && line.contains("hello") && line.contains("42"));
        assert!(line.contains("main=true scheduler=false audio=false"));
    }
    k.send(obj, "low", &args).unwrap();
    assert!(k.main_queue_len() > 0);
    assert!(k.free_object(obj));
    k.run_main_queue();
    assert!(posts(&k).is_empty());
    assert_eq!(k.live_object_count(), 0);
}

#[test]
fn overdrive_off_front_runs_in_main_scheduler_context_but_low_waits() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("median.validate", &["fixture".into()]);
    assert!(!obj.is_null());
    k.with_role(Role::MainTimer, || {
        k.send(obj, "direct", &[Atom::Long(32)]).unwrap();
        k.send(obj, "low", &[Atom::Long(32)]).unwrap();
        k.send(obj, "front", &[Atom::Long(32)]).unwrap();
        assert_eq!(
            posts(&k),
            [
                "fixture: direct 32; main=true scheduler=true audio=false",
                "fixture: front 32; main=true scheduler=true audio=false",
            ]
        );
    });
    assert!(k.main_queue_len() > 0);
    k.run_main_queue();
    assert_eq!(
        posts(&k),
        ["fixture: low 32; main=true scheduler=false audio=false"]
    );
    assert!(k.free_object(obj));
    assert_eq!(k.live_object_count(), 0);
}

#[test]
fn mixed_msp_inlets_pass_both_signals_and_report_custom_messages() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("median.validate~", &[]);
    assert!(!obj.is_null());
    assert_eq!(k.signal_inlets(obj), Some(2));
    assert_eq!(
        k.assist(obj, false, 2).as_deref(),
        Some("2: integer, reported in console")
    );
    assert_eq!(
        k.assist(obj, false, 3).as_deref(),
        Some("3: proxy; send probe")
    );
    k.send_inlet(obj, 2, "int", &[Atom::Long(17)]).unwrap();
    k.send_inlet(obj, 3, "probe", &[]).unwrap();
    let log = posts(&k);
    assert!(log[0].contains("integer inlet 2 value 17"));
    assert!(log[1].contains("proxy inlet 3"));
    let adds = k.dsp64(obj, 48000., 4).unwrap();
    let mut ins = vec![vec![0.125; 4], vec![0.75; 4]];
    let mut outs = vec![vec![0.; 4], vec![0.; 4]];
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(ins, outs);
    assert!(posts(&k).is_empty()); // perform must not post or send messages.
    assert!(k.free_object(obj));
    assert_eq!(k.live_object_count(), 0);
}

#[test]
fn jitter_probes_report_physical_positions_and_refusal_cleans_up() {
    let k = median_mock::lock();
    register();
    let matrix = k.new_object("jit.median.validate", &[]);
    let gl = k.new_object("jit.gl.median.validate", &["validation".into()]);
    assert!(!matrix.is_null() && !gl.is_null());
    assert_eq!(k.outlet_count(matrix), 5);
    assert_eq!(k.outlet_count(gl), 3);
    assert_eq!(k.inlet_is_hot(matrix, 2), Some(false));
    assert_eq!(k.inlet_is_hot(matrix, 3), Some(true));
    assert_eq!(k.inlet_is_hot(matrix, 4), Some(false));
    assert_eq!(k.inlet_is_hot(gl, 1), Some(true));
    k.send_inlet(matrix, 2, "int", &[Atom::Long(19)]).unwrap();
    k.send_inlet(matrix, 3, "float", &[Atom::Float(2.5)])
        .unwrap();
    k.send_inlet(matrix, 4, "probe", &[]).unwrap();
    k.send_inlet(gl, 1, "probe", &[]).unwrap();
    let output = k.take_outlet_log();
    assert_eq!(output.len(), 3);
    assert_eq!(
        (output[0].index, &output[0].atoms),
        (3, &vec![Atom::Long(19)])
    );
    assert_eq!(output[1].index, 4);
    assert_eq!(
        (output[2].index, &output[2].atoms),
        (2, &vec![Atom::Long(1)])
    );
    let log = posts(&k);
    assert!(log[0].contains("float inlet 3 value 2.5"));
    assert!(log[1].contains("proxy inlet 4"));
    assert!(log[2].contains("proxy inlet 1"));
    assert!(k.free_object(matrix));
    assert!(k.free_object(gl));
    for _ in 0..3 {
        assert!(
            k.new_object("jit.median.validate", &["refuse".into()])
                .is_null()
        );
        assert!(
            posts(&k)
                .iter()
                .any(|s| s.contains("intentional constructor refusal"))
        );
        assert_eq!(k.live_object_count(), 0);
        assert_eq!(median_mock::jit::live_mop_count(), 0);
        assert_eq!(median_mock::jit::context_count(), 0);
    }
}
