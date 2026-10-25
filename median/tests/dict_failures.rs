//! Failure scenarios test wrapper ownership, not the host's undocumented rollback behavior.
use median::{
    atom::Atom,
    dict::{AtomArray, Dict, DictError, DictValue},
    sym,
};
use median_mock::containers::{Failure, fail_next, live_count};

#[test]
fn failed_allocations_and_clones_leave_existing_containers_owned() {
    let _kernel = median_mock::lock();
    fail_next(Failure::DictionaryNew);
    assert!(matches!(Dict::new(), Err(DictError::Allocation)));
    fail_next(Failure::AtomArrayNew);
    assert!(matches!(AtomArray::new(&[]), Err(DictError::Allocation)));
    assert_eq!(live_count(), 0);

    let d = Dict::new().unwrap();
    d.insert(sym!("value"), 42i64).unwrap();
    fail_next(Failure::DictionaryClone);
    assert!(matches!(d.deep_clone(), Err(DictError::Allocation)));
    assert!(matches!(
        d.get(sym!("value")).unwrap(),
        Some(DictValue::Int(42))
    ));
    // Failure must also release the source transaction lock, not merely keep it alive.
    std::thread::scope(|s| {
        s.spawn(|| d.insert(sym!("value"), 43i64).unwrap())
            .join()
            .unwrap();
    });
    assert_eq!(live_count(), 1);

    let array = AtomArray::new(&[7i64.into()]).unwrap();
    fail_next(Failure::AtomArrayNew);
    assert!(matches!(array.try_clone(), Err(DictError::Allocation)));
    assert_eq!(array.atoms().unwrap()[0].get_int(), 7);
    assert_eq!(live_count(), 2);
    drop(array);
    drop(d);
    assert_eq!(live_count(), 0);
}

#[test]
fn failed_array_growth_and_dictionary_insertion_can_be_retried() {
    let _kernel = median_mock::lock();
    let mut array = AtomArray::new(&[1i64.into()]).unwrap();
    fail_next(Failure::AtomArraySetAtoms);
    assert!(array.push(2i64.into()).is_err());
    // This injected failure happens before mutation; no general SDK rollback is promised.
    assert_eq!(array.atoms().unwrap()[0].get_int(), 1);
    assert_eq!(array.len(), 1);
    array.push(2i64.into()).unwrap();
    assert_eq!(array.len(), 2);

    let d = Dict::new().unwrap();
    let values = [Atom::from(9i64)];
    fail_next(Failure::AtomArrayNew);
    assert!(d.insert(sym!("array"), values.as_slice()).is_err());
    assert!(!d.contains_key(sym!("array")));
    assert_eq!(live_count(), 2);
    d.insert(sym!("array"), values.as_slice()).unwrap();
    assert_eq!(live_count(), 3);
    drop(array);
    drop(d);
    assert_eq!(live_count(), 0);
}

#[test]
fn post_store_append_errors_do_not_release_transferred_children_twice() {
    let _kernel = median_mock::lock();
    let d = Dict::new().unwrap();
    d.insert(sym!("child"), Dict::new().unwrap()).unwrap();
    let child = Dict::new().unwrap();
    child.insert(sym!("value"), 42i64).unwrap();
    fail_next(Failure::AppendDictionaryAfterStore);
    assert!(d.insert(sym!("child"), child).is_err());
    assert_eq!(live_count(), 2); // old child released; replacement belongs to d
    let Some(DictValue::Dict(retained)) = d.get(sym!("child")).unwrap() else {
        panic!("missing child")
    };
    assert!(matches!(
        retained.get(sym!("value")).unwrap(),
        Some(DictValue::Int(42))
    ));

    d.insert(sym!("array"), AtomArray::new(&[]).unwrap())
        .unwrap();
    let array = AtomArray::new(&[7i64.into()]).unwrap();
    fail_next(Failure::AppendAtomArrayAfterStore);
    assert!(d.insert(sym!("array"), array).is_err());
    assert_eq!(live_count(), 3);
    let Some(DictValue::Atoms(values)) = d.get(sym!("array")).unwrap() else {
        panic!("missing array")
    };
    assert_eq!(values[0].get_int(), 7);
    drop(d);
    assert_eq!(live_count(), 1); // explicitly retained child survives the parent
    drop(retained);
    assert_eq!(live_count(), 0);
}
