//! The special `A_CANT` messages `#[dblclick]` and `#[loadbang]` (`Class::add_dblclick`,
//! `Class::add_loadbang`), called through `median-mock` as Max calls them.

use median::prelude::*;
use median_mock::Level;
use std::sync::Once;

#[derive(MaxObj)]
#[max(name = "special.thing")]
pub struct Thing {
    obj: ObjRef,
}

impl MaxObjWrapped for Thing {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { obj: b.obj() })
    }
}

#[median::methods]
impl Thing {
    #[dblclick]
    pub fn open(&self) {
        self.obj.post("double-clicked");
    }

    #[loadbang]
    pub fn loaded(&self) -> Result<(), &'static str> {
        self.obj.post("loaded");
        Err("and failed")
    }
}

median::ext_main! { Thing }

fn register() {
    static ONCE: Once = Once::new();
    // SAFETY: called once, as Max does
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
}

#[test]
fn dblclick_and_loadbang_dispatch() {
    let k = median_mock::lock();
    register();
    let obj = k.new_object("special.thing", &[]);
    assert!(!obj.is_null());
    let a_cant = median::max_sys::e_max_atomtypes::A_CANT as i32;
    assert_eq!(k.method(obj, "dblclick").unwrap().1, [a_cant]);
    assert_eq!(k.method(obj, "loadbang").unwrap().1, [a_cant]);
    //A_CANT: not sendable as a message
    assert!(k.send(obj, "loadbang", &[]).is_err());
    k.take_console_log();

    assert!(k.dblclick(obj));
    assert!(k.loadbang(obj));
    let log: Vec<_> = k
        .take_console_log()
        .into_iter()
        .map(|e| (e.level, e.object, e.text))
        .collect();
    let o = obj as usize;
    assert_eq!(
        log,
        [
            (Level::Post, o, "double-clicked".to_string()),
            (Level::Post, o, "loaded".to_string()),
            (Level::Error, o, "and failed".to_string()),
        ]
    );
    assert!(k.free_object(obj));
}
