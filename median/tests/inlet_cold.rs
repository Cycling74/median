//! `Inlet::cold` and `Inlet::hot_if`: inlet 0 can be declared cold (a control inlet, like
//! `gate`'s left one), hot/cold can be computed, and the last of `hot`/`cold`/`hot_if` on one
//! description wins. Checked through `inletinfo` in the
//! `median-mock` kernel.

use median::prelude::*;

#[derive(MaxObject)]
#[max(name = "io.cold", no_methods)]
pub struct IoCold {
    _obj: ObjRef,
}

impl MaxObjWrapped for IoCold {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::default().assist("control").cold());
        b.inlet(Inlet::proxy().hot());
        b.inlet(Inlet::proxy().hot().cold());
        b.inlet(Inlet::proxy());
        //computed: positions 4..=7, the even ones hot
        for i in 4..8 {
            b.inlet(Inlet::proxy().hot_if(i % 2 == 0));
        }
        //`hot_if(false)` makes a marked inlet cold again
        b.inlet(Inlet::proxy().hot().hot_if(false));
        Ok(Self { _obj: b.obj() })
    }
}

#[test]
fn inlet_0_can_be_cold_and_the_last_marker_wins() {
    let k = median_mock::lock();
    IoCold::register();
    let obj = k.new_object("io.cold", &[]);
    assert!(!obj.is_null());
    let hot: Vec<_> = (0..9).map(|i| k.inlet_is_hot(obj, i).unwrap()).collect();
    assert_eq!(hot, [false, true, false, false, true, false, true, false, false]);
    assert!(k.free_object(obj));
}
