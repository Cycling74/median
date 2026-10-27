//! Exercise the exact diagnostic external shipped with the Max validation patches.
#[path = "../examples/validation/src/lib.rs"]
mod validation;

use median::dict::{Dict, Ownership};
use median::wrapper::{MSPObjWrapper, WrapperWrapped};
use median_mock::{Atom, CallReply, Kernel, Role, SendError, containers};
use validation::buffer_probe::BufferProbe;

fn register() {
    // SAFETY: called as Max calls ext_main, while holding the mock kernel lock.
    unsafe { validation::ext_main(std::ptr::null_mut()) };
}

fn posts(k: &Kernel) -> Vec<String> {
    k.take_console_log().into_iter().map(|e| e.text).collect()
}

#[test]
fn buffer_probe_checks_exclusion_and_detects_a_counterexample_without_touching_samples() {
    let k = median_mock::lock();
    register();
    k.create_buffer("probe", 2, 32, 44100.);
    let samples: Vec<_> = (0..64).map(|i| i as f32 / 64.).collect();
    k.set_buffer_samples("probe", &samples);
    let obj = k.new_object("median.validate.buffer~", &["probe".into()]);
    assert!(!obj.is_null());
    let probe = unsafe { &*obj.cast::<MSPObjWrapper<BufferProbe>>() }.wrapped();
    let dsp = k.dsp64(obj, 44100., 64).unwrap();
    let mut outs = vec![vec![1.; 64]];
    for exclusion in [true, false, true] {
        assert!(median_mock::buffer::set_edit_exclusion("probe", exclusion));
        probe.probe_with(|| dsp[0].run(&mut [], &mut outs)).unwrap();
        let log = posts(&k);
        assert_eq!(log.len(), 4, "{log:?}");
        let expected = if exclusion { "PASS" } else { "FAIL" };
        assert!(log[2].contains(&format!("main_exclusion={expected} pointer_identity=PASS")));
        assert!(log[3].contains(&format!("audio_exclusion={expected}")));
        assert_eq!(outs[0], vec![0.; 64]);
        assert_eq!(k.buffer_samples("probe").unwrap(), samples);
        assert_eq!(k.buffer_state("probe"), Some((0, false)));
        assert_eq!(k.buffer_editing("probe"), Some(false));
        assert_eq!(k.buffer_last_edit_valid("probe"), Some(Some(-1)));
    }
    assert_eq!(k.buffer_edits("probe"), Some((3, 3)));
    assert!(k.free_object(obj));
    assert!(posts(&k).is_empty());
}

#[test]
fn buffer_probe_defers_to_main_reports_dsp_off_and_recovers_after_unwind() {
    let k = median_mock::lock();
    register();
    k.create_buffer("probe", 1, 16, 44100.);
    let obj = k.new_object("median.validate.buffer~", &["probe".into()]);
    assert!(!obj.is_null());
    let probe = unsafe { &*obj.cast::<MSPObjWrapper<BufferProbe>>() }.wrapped();
    for role in [Role::Timer, Role::Audio, Role::MainTimer, Role::Other] {
        assert!(k.with_role(role, || probe.probe_with(|| {})).is_err());
    }
    assert_eq!(k.buffer_edits("probe"), Some((0, 0)));
    let mut window = 0;
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = probe.probe_with(|| {
            window += 1;
            if window == 2 {
                panic!("inside edit observation");
            }
        });
    }));
    assert!(panic.is_err());
    assert_eq!(k.buffer_editing("probe"), Some(false));
    assert_eq!(k.buffer_edits("probe"), Some((1, 1)));
    k.with_role(Role::Timer, || k.send(obj, "probe", &[]).unwrap());
    assert!(posts(&k).is_empty());
    k.run_main_queue();
    let log = posts(&k);
    assert_eq!(log.len(), 4, "{log:?}");
    assert!(log[0].contains("main=true scheduler=false audio=false"));
    assert!(log[3].contains("audio_exclusion=INCONCLUSIVE"));
    assert_eq!(k.buffer_edits("probe"), Some((2, 2)));
    k.destroy_buffer("probe");
    assert!(
        probe
            .probe_with(|| {})
            .unwrap_err()
            .contains("does not exist")
    );
    assert!(k.free_object(obj));
    assert!(posts(&k).is_empty());
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
    assert!(posts(&k).is_empty()); // Includes overlapping Max/Jitter storage diagnostics.
    for obj in [matrix, gl] {
        let core = median_mock::jit::object_for(obj);
        assert!(!core.is_null() && core != obj);
        // Max performs obex lookups after the constructor returns. Both the Max table
        // and Jitter's explicit core-to-wrapper attachment must survive construction.
        unsafe {
            let mut owner = std::ptr::null_mut();
            assert_eq!(
                median::max_sys::object_obex_lookup(
                    core,
                    median::sym!("maxwrapper").as_ptr(),
                    &mut owner,
                ),
                0
            );
            assert_eq!(owner.cast::<core::ffi::c_void>(), obj);
            let key = median::sym!("validation.storage").as_ptr();
            assert_eq!(
                median::max_sys::object_obex_storeflags(obj, key, core.cast(), 1),
                0
            );
            assert_eq!(median::max_sys::object_obex_lookup(obj, key, &mut owner), 0);
            assert_eq!(owner.cast::<core::ffi::c_void>(), core);
        }
    }
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

#[test]
fn messages_gimmeback_methods_reply_to_typed_callers_and_refuse_patch_cords() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("median.validate", &["gb".into()]);
    assert!(!obj.is_null());
    let live = containers::live_count();
    let args = [Atom::Long(1), Atom::Float(2.5), Atom::Sym("foo".into())];
    assert_eq!(k.call(obj, "getcount", &args), Ok(CallReply::Int(3)));
    assert_eq!(
        posts(&k),
        ["gb: getcount 3; main=true scheduler=false audio=false"]
    );
    assert_eq!(
        k.call(obj, "getlist", &args),
        Ok(CallReply::Atoms(vec![
            Atom::Long(2),
            Atom::Float(5.),
            Atom::Sym("foo".into())
        ]))
    );
    assert_eq!(
        posts(&k),
        ["gb: getlist 3; main=true scheduler=false audio=false"]
    );
    match k.call(obj, "getstate", &args) {
        Ok(CallReply::Object(p)) => {
            let d = unsafe { Dict::from_raw(p.cast(), Ownership::Retained) }.unwrap();
            assert_eq!(
                d.get_sym(median::sym!("label"))
                    .unwrap()
                    .map(|s| s.to_string()),
                Some("gb".to_owned())
            );
            assert_eq!(d.get_int(median::sym!("count")).unwrap(), Some(3));
            drop(d);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        posts(&k),
        ["gb: getstate 3; main=true scheduler=false audio=false"]
    );
    assert!(matches!(
        k.call(obj, "getfail", &args),
        Ok(CallReply::Error(_))
    ));
    let log = posts(&k);
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(log[0].contains("getfail 3"));
    assert!(log[1].contains("getfail: intentional error"));
    assert_eq!(containers::live_count(), live, "a reply was leaked");
    // a patch cord does not reach any of them
    // `getquiet` is a list of 4096 ints and posts nothing, for the leak check
    match k.call(obj, "getquiet", &args) {
        Ok(CallReply::Atoms(list)) => {
            assert_eq!(list.len(), 4096);
            assert_eq!(list[0], Atom::Long(0));
            assert_eq!(list[4095], Atom::Long(4095));
        }
        other => panic!("getquiet: {other:?}"),
    }
    assert!(
        k.take_console_log().is_empty(),
        "getquiet must post nothing"
    );
    assert_eq!(containers::live_count(), live, "getquiet leaked a reply");
    for sel in ["getcount", "getlist", "getstate", "getquiet", "getfail"] {
        assert_eq!(
            k.send(obj, sel, &args),
            Err(SendError::TypedOnly(sel.into()))
        );
    }
    assert!(k.free_object(obj));
}

#[test]
fn matrix_gimmeback_methods_reply_out_the_dumpout_outlet() {
    let k = median_mock::lock();
    register();
    assert_eq!(median_mock::jit::wrap_flags("jit.median.validate"), Some(1));
    let obj = k.new_object("jit.median.validate", &[]);
    assert!(!obj.is_null());
    posts(&k);
    let live = containers::live_count();
    k.take_outlet_log();
    k.send(obj, "getvalues", &[Atom::Long(7)]).unwrap();
    let out = k.take_outlet_log();
    assert_eq!(out.len(), 1, "{out:?}");
    // dumpout is SDK outlet 2 (matrix outlets 0-1), left of the custom outlets 3 and 4
    assert_eq!(out[0].index, 2);
    // the reply goes out without the leading `get` (observed in Max 9.2, 2026-10-27)
    assert_eq!(out[0].selector, "values");
    assert_eq!(
        out[0].atoms,
        [
            Atom::Long(1),
            Atom::Float(2.5),
            Atom::Sym("foo".into()),
            Atom::Long(7)
        ]
    );
    k.send(
        obj,
        "getcount",
        &[Atom::Long(1), Atom::Long(2), Atom::Long(3)],
    )
    .unwrap();
    let out = k.take_outlet_log();
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(
        (out[0].index, out[0].selector.as_str(), &out[0].atoms),
        (2, "count", &vec![Atom::Long(3)])
    );
    assert_eq!(containers::live_count(), live, "an atomarray is left over");
    let log = posts(&k);
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(log[0].contains("getvalues 1") && log[1].contains("getcount 3"));
    // `getbig` is a list of 4096 ints out dumpout under `big`, and posts nothing (the burst
    // test of patch 05 part C)
    k.send(obj, "getbig", &[]).unwrap();
    let out = k.take_outlet_log();
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!((out[0].index, out[0].selector.as_str()), (2, "big"));
    assert_eq!(out[0].atoms.len(), 4096);
    assert_eq!(out[0].atoms[0], Atom::Long(0));
    assert_eq!(out[0].atoms[4095], Atom::Long(4095));
    assert_eq!(containers::live_count(), live, "getbig leaked a reply");
    assert!(posts(&k).is_empty(), "getbig must post nothing");
    assert!(k.free_object(obj));
}
