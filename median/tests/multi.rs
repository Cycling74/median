//! The `multi` example: two classes (a Max and an MSP one) registered by one `ext_main!`.

#[path = "../examples/multi/src/lib.rs"]
mod multi;

use median_mock::Level;

#[test]
fn one_ext_main_registers_both_classes() {
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    unsafe { multi::ext_main(std::ptr::null_mut()) };
    assert!(k.class_exists("median.multi"));
    assert!(k.class_exists("median.multi.sig~"));

    let base = k.new_object("median.multi", &[]);
    let sig = k.new_object("median.multi.sig~", &[]);
    assert!(!base.is_null() && !sig.is_null());
    let posts: Vec<_> = k
        .take_console_log()
        .into_iter()
        .filter(|e| e.level == Level::Post)
        .map(|e| e.text)
        .collect();
    assert_eq!(posts, ["created median.multi", "created median.multi.sig~"]);

    //the MSP one passes its two inputs through
    assert_eq!(k.signal_inlets(sig), Some(2));
    let adds = k.dsp64(sig, 48000., 2).unwrap();
    let mut ins = vec![vec![1., 2.], vec![3., 4.]];
    let mut outs = vec![vec![0.; 2], vec![0.; 2]];
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(outs, ins);

    assert!(k.free_object(base));
    assert!(k.free_object(sig));
    assert_eq!(k.live_object_count(), 0);
}
