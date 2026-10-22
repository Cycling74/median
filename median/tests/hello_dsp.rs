//! The `hello_dsp` example: an MSP object with a field attribute and a `#[sel]` method.

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
    assert_eq!((c[0].level, c[0].text.as_str()), (Level::Post, "sample rate: 48000"));
    let mut ins = vec![vec![0.; 2], vec![0.; 2]];
    let mut outs = vec![vec![9.; 2], vec![9.; 2]];
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(outs, [[0., 0.], [1., 1.]]);
    assert!(k.free_object(obj));
}
