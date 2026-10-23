use median::{dict::DictResult, prelude::*};

#[derive(MaxObject)]
#[max(name = "test.dictionary")]
struct Receiver {
    state: Dict,
    out: Outlet<Any>,
}
impl MaxObjWrapped for Receiver {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            state: Dict::new().map_err(|e| NewError::from(e.to_string()))?,
            out: b.outlet(Outlet::any()),
        })
    }
}
#[median::methods]
impl Receiver {
    #[dictionary]
    fn receive(&self, dictionary: Dict) -> DictResult<()> {
        if dictionary.contains_key(sym!("panic")) {
            panic!("dictionary callback panic");
        }
        dictionary.insert(sym!("received"), 1)?;
        self.state.insert(sym!("data"), &dictionary)?;
        self.out.send_dict(&dictionary).unwrap();
        Ok(())
    }
    #[savestate]
    fn save(&self, patcher: &Dict) -> DictResult<()> {
        patcher.insert(sym!("saved"), &self.state)
    }
}
median::ext_main! { Receiver }

#[test]
fn dictionary_receive_send_and_savestate_through_registered_trampolines() {
    let k = median_mock::lock();
    unsafe {
        ext_main(std::ptr::null_mut());
    }
    let obj = k.new_object("test.dictionary", &[]);
    assert!(!obj.is_null());
    let (d, name) = Dict::new().unwrap().register(None).unwrap();
    d.insert(sym!("x"), 42).unwrap();
    k.send(obj, "dictionary", &[name.to_str().unwrap().into()])
        .unwrap();
    assert!(d.contains_key(sym!("received")));
    let messages = k.take_outlet_log();
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].atoms,
        [median_mock::Atom::Sym(name.to_string())]
    );
    let patcher = Dict::new().unwrap();
    let (f, types) = k.method(obj, "appendtodictionary").unwrap();
    assert_eq!(types, [max_sys::e_max_atomtypes::A_CANT as i32]);
    let save: unsafe extern "C" fn(*mut core::ffi::c_void, *mut max_sys::t_dictionary) =
        unsafe { std::mem::transmute(f) };
    unsafe {
        save(obj, patcher.as_ptr());
    }
    d.insert(sym!("x"), 99).unwrap();
    assert!(matches!(
        patcher.get_path("saved::data::x").unwrap(),
        Some(DictValue::Int(42))
    ));
    k.send(obj, "dictionary", &["not-registered".into()])
        .unwrap();
    assert!(
        k.take_console_log()
            .iter()
            .any(|e| e.text.contains("unable to reference dictionary"))
    );
    d.insert(sym!("panic"), 1).unwrap();
    k.send(obj, "dictionary", &[name.to_str().unwrap().into()])
        .unwrap();
    assert!(
        k.take_console_log()
            .iter()
            .any(|e| e.text.contains("dictionary callback panic"))
    );
    // Panic has dropped the trampoline's temporary dictionary retain.
    drop(d);
    assert!(Dict::find_registered(name).is_none());
    unsafe {
        save(obj, std::ptr::null_mut());
    }
    assert!(!k.take_console_log().is_empty());
    assert!(k.free_object(obj));
    drop(patcher);
    assert_eq!(median_mock::containers::live_count(), 0);
}
