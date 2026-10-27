use median::{
    dict::{Dict, DictError, DictValue, ObjectFlags},
    file::FilePath,
    prelude::*,
};
use std::ffi::CString;

#[test]
fn parsing_files_and_constructor_errors_keep_ownership_balanced() {
    let _k = median_mock::lock();
    let baseline = median_mock::containers::live_count();
    {
        let d =
            Dict::from_json(r#"{"n":7,"nested":{"label":"hello"},"values":[1,2.5,"x"]}"#).unwrap();
        assert_eq!(d.get_int(sym!("n")).unwrap(), Some(7));
        assert!(
            matches!(d.get_path("nested::label").unwrap(), Some(DictValue::Str(s)) if s.to_bytes() == b"hello")
        );
        let a = d.get_atoms(sym!("values")).unwrap().unwrap();
        assert_eq!(a.len(), 3);
        assert_eq!(
            unsafe { d.with_atoms_raw(sym!("values"), |a| a[1].get_float()) }.unwrap(),
            Some(2.5)
        );
        assert!(
            unsafe { d.with_atoms_raw(sym!("n"), |_| ()) }
                .unwrap()
                .is_none()
        );
        let parsed = Dict::from_atoms(&[sym!("value:").into(), 42i64.into()]).unwrap();
        assert_eq!(parsed.get_int(sym!("value")).unwrap(), Some(42));
        let name =
            std::env::temp_dir().join(format!("median-dict-helpers-{}.json", std::process::id()));
        let path = FilePath {
            file_name: CString::new(name.to_str().unwrap()).unwrap(),
            vol: 0,
            typ: 0,
        };
        d.write(&path).unwrap();
        let copy = Dict::read(&path).unwrap();
        assert_eq!(copy.get_int(sym!("n")).unwrap(), Some(7));
        assert!(d.dump(true, true).is_ok());
        std::fs::remove_file(name).unwrap();
        assert!(Dict::read(&path).is_err());
        for partial in [false, true] {
            let before = median_mock::containers::live_count();
            median_mock::containers::fail_next_constructor(partial);
            assert!(Dict::from_json(r#"{"child":{"a":1}}"#).is_err());
            assert_eq!(median_mock::containers::live_count(), before);
        }
        assert!(Dict::from_json("not JSON").is_err());
        assert_eq!(
            Dict::from_json("{}\0").unwrap_err(),
            DictError::InvalidString
        );
        let atom = unsafe { Atom::from_object_raw(d.as_ptr().cast()) };
        assert_eq!(
            Dict::from_atoms(&[atom]).unwrap_err(),
            DictError::ObjectAtom
        );
        let atom = unsafe { Atom::from_object_raw(d.as_ptr().cast()) };
        let retained = unsafe { atom.as_dict_unchecked() }.unwrap().unwrap();
        assert!(d.ptr_eq(&retained));
        assert!(unsafe { atom.as_atomarray_unchecked() }.unwrap().is_none());
        d.transaction(|d| unsafe {
            let mut array = std::ptr::null_mut();
            assert_eq!(
                max_sys::dictionary_getatomarray(d.as_ptr(), sym!("values").as_ptr(), &mut array),
                0
            );
            let atom = Atom::from_object_raw(array.cast());
            let copy = atom.as_atomarray_unchecked().unwrap().unwrap();
            assert_eq!(copy[0].get_int(), 1);
            assert_eq!(copy[1].get_float(), 2.5);
            assert!(atom.as_dict_unchecked().unwrap().is_none());
        });
    }
    assert_eq!(median_mock::containers::live_count(), baseline);
}

#[test]
fn name_switches_and_weak_entry_removal_do_not_release_another_owner() {
    let _k = median_mock::lock();
    let (original, _) = Dict::new()
        .unwrap()
        .register(Some(sym!("original")))
        .unwrap();
    original.insert(sym!("n"), 12).unwrap();
    let same = original.rename(sym!("original")).unwrap();
    assert!(original.ptr_eq(&same));
    let next = original.rename(sym!("next")).unwrap();
    assert!(next.is_empty());
    assert_eq!(original.get_int(sym!("n")).unwrap(), Some(12));
    assert!(next.ptr_eq(&original.rename(sym!("next")).unwrap()));
    let parent = Dict::new().unwrap();
    for flags in [ObjectFlags::Reference, ObjectFlags::Data] {
        unsafe {
            parent
                .insert_object(sym!("weak"), original.as_ptr().cast(), flags)
                .unwrap();
        }
        assert!(matches!(
            parent.remove(sym!("weak")),
            Err(DictError::OpaqueObject)
        ));
        assert!(parent.delete(sym!("weak")).unwrap());
        assert_eq!(original.get_int(sym!("n")).unwrap(), Some(12));
    }
    drop((parent, next, same, original));
    assert_eq!(median_mock::containers::live_count(), 0);
}

#[derive(MaxObject)]
#[max(name = "dict.helpers.observer", no_methods)]
struct Observer;
impl MaxObjWrapped for Observer {
    fn new(_: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self)
    }
    fn handle_notification(&self, n: &median::notify::Notification) {
        if *n.message() == sym!("modified") {
            // Reentrant read/write confirms notify_modified holds no dictionary lock.
            Dict::find_registered(sym!("observed"))
                .unwrap()
                .insert(sym!("notified"), 1)
                .unwrap();
        }
    }
}
#[test]
fn modified_notifications_reach_subscribers() {
    let k = median_mock::lock();
    Observer::register();
    let (dict, _) = Dict::new()
        .unwrap()
        .register(Some(sym!("observed")))
        .unwrap();
    let observer = k.new_object("dict.helpers.observer", &[]);
    assert!(!observer.is_null());
    let subscription = unsafe {
        median::notify::Subscription::new(
            observer.cast(),
            sym!("dictionary"),
            sym!("observed"),
            None,
        )
    };
    dict.notify_modified().unwrap();
    assert_eq!(dict.get_int(sym!("notified")).unwrap(), Some(1));
    drop(subscription);
    assert!(k.free_object(observer));
}

#[test]
fn another_class_at_the_requested_name_consumes_the_rejected_dictionary() {
    let k = median_mock::lock();
    Observer::register();
    let object = k.new_object("dict.helpers.observer", &[]);
    assert!(!object.is_null());
    unsafe {
        max_sys::object_register(
            sym!("dictionary").as_ptr(),
            sym!("occupied").as_ptr(),
            object,
        );
    }
    let before = median_mock::containers::live_count();
    assert!(matches!(
        Dict::new().unwrap().register(Some(sym!("occupied"))),
        Err(median::dict::RegisterError::NameTakenByOtherClass)
    ));
    assert_eq!(median_mock::containers::live_count(), before);
    assert!(k.free_object(object));
}

#[test]
fn a_foreign_object_registered_as_a_dictionary_name_is_not_a_dictionary() {
    let k = median_mock::lock();
    Observer::register();
    let object = k.new_object("dict.helpers.observer", &[]);
    assert!(!object.is_null());
    unsafe {
        max_sys::object_register(
            sym!("dictionary").as_ptr(),
            sym!("foreign").as_ptr(),
            object,
        );
    }
    let refs = k.retain_count(object);
    assert_eq!(refs, Some(0));
    assert!(Dict::find_registered(sym!("foreign")).is_none());
    assert!(
        Dict::find_registered_clone(sym!("foreign"))
            .unwrap()
            .is_none()
    );
    // retained by the lookup, released again when it was refused
    assert_eq!(k.retain_count(object), refs);
    assert!(k.free_object(object));
}
