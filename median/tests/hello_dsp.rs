//! The `hello_dsp` example: an MSP object with a field attribute, methods, and a buffer it plays
//! (with `set`/`set2` methods and a `#[dblclick]` that opens it).

#[path = "../examples/hello_dsp/src/lib.rs"]
mod hello_dsp;

use median_mock::{Atom, Level};

#[test]
fn hello_dsp_performs_and_has_its_attribute_and_methods() {
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { hello_dsp::ext_main(std::ptr::null_mut()) };
    let obj = k.new_object("hello_dsp~", &[]);
    assert!(!obj.is_null());
    assert_eq!(k.signal_inlets(obj), Some(2));
    assert_eq!(k.outlet_count(obj), 2);
    for sel in ["bang", "int", "set"] {
        assert!(k.method(obj, sel).is_some(), "{sel}");
    }
    assert_eq!(k.attr_names(obj), ["blah"]);

    k.send_int(obj, 3).unwrap();
    assert_eq!(k.attr_get(obj, "blah").unwrap(), [Atom::Long(3)]);
    k.attr_set(obj, "blah", &[Atom::Long(-1)]).unwrap();
    k.take_console_log();
    //no buffers named: the first outlet is 0, the second 1
    k.send(obj, "set", &[Atom::Sym("nonesuch".into())]).unwrap();
    let adds = k.dsp64(obj, 48000., 2).unwrap();
    let c = k.take_console_log();
    assert_eq!(c.len(), 1);
    assert_eq!(
        (c[0].level, c[0].text.as_str()),
        (Level::Post, "sample rate: 48000")
    );
    let mut ins = vec![vec![0.; 2], vec![0.; 2]];
    let mut outs = vec![vec![9.; 2], vec![9.; 2]];
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(outs, [[0., 0.], [1., 1.]]);
    //with a buffer: the right outlet plays its first channel (the data is interleaved), repeated
    //over the vector; the left counts the buffers that exist
    k.create_buffer("tbl", 2, 3, 44100.);
    k.set_buffer_samples("tbl", &[1., -1., 2., -2., 3., -3.]);
    k.send(obj, "set", &[Atom::Sym("tbl".into())]).unwrap();
    let mut ins = vec![vec![0.; 4], vec![0.; 4]];
    let mut outs = vec![vec![9.; 4], vec![9.; 4]];
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(outs, [[1., 1., 1., 1.], [1., 2., 3., 1.]]);
    assert_eq!(k.buffer_state("tbl"), Some((0, false)));
    //double-click opens it
    assert!(k.dblclick(obj));
    assert_eq!(k.buffer_views(), ["tbl"]);
    assert!(k.take_console_log().is_empty());
    assert!(k.free_object(obj));
}

#[test]
fn hello_dsp_buffers_come_from_its_arguments_and_set_set2() {
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { hello_dsp::ext_main(std::ptr::null_mut()) };
    k.create_buffer("a", 1, 2, 44100.);
    k.set_buffer_samples("a", &[0.5, 0.25]);
    k.create_buffer("b", 1, 2, 44100.);
    k.create_buffer("c", 1, 2, 44100.);
    k.set_buffer_samples("c", &[-1., -2.]);

    //run one vector of 2 and return the two outlets
    let run = |obj| {
        let adds = k.dsp64(obj, 48000., 2).unwrap();
        let mut ins = vec![vec![0.; 2], vec![0.; 2]];
        let mut outs = vec![vec![9.; 2], vec![9.; 2]];
        adds[0].run(&mut ins, &mut outs);
        outs
    };

    //`[hello_dsp~ a b @blah 4]`: both buffers from the arguments; `@blah` ends the names, and
    //the attribute is set after `new`
    let args = [
        Atom::Sym("a".into()),
        Atom::Sym("b".into()),
        Atom::Sym("@blah".into()),
        Atom::Long(4),
    ];
    let obj = k.new_object("hello_dsp~", &args);
    assert!(!obj.is_null());
    assert_eq!(k.attr_get(obj, "blah").unwrap(), [Atom::Long(4)]);
    assert_eq!(run(obj), [[2., 2.], [0.5, 0.25]]);

    //`set2` repoints buffer2 only: at a buffer~ that doesn't exist, then at one that does
    k.send(obj, "set2", &[Atom::Sym("nonesuch".into())])
        .unwrap();
    assert_eq!(run(obj), [[1., 1.], [0.5, 0.25]]);
    k.send(obj, "set2", &[Atom::Sym("c".into())]).unwrap();
    //`set` repoints buffer1, which the right outlet plays
    k.send(obj, "set", &[Atom::Sym("c".into())]).unwrap();
    assert_eq!(run(obj), [[2., 2.], [-1., -2.]]);
    assert!(k.free_object(obj));

    //one argument: only buffer1; a number ends the names
    let obj = k.new_object("hello_dsp~", &[Atom::Sym("a".into()), Atom::Long(3)]);
    assert!(!obj.is_null());
    assert_eq!(run(obj), [[1., 1.], [0.5, 0.25]]);
    assert!(k.free_object(obj));
}
