use median::{
    atom::Atom,
    dict::{AtomArray, Dict, DictError, DictValue, RegisterError},
    sym,
};

fn int(d: &Dict, key: median::symbol::SymbolRef) -> i64 {
    match d.get(key).unwrap().unwrap() {
        DictValue::Int(v) => v as i64,
        _ => panic!("not an int"),
    }
}
fn child(d: &Dict) -> Dict {
    match d.get(sym!("child")).unwrap().unwrap() {
        DictValue::Dict(v) => v,
        _ => panic!("not a dictionary"),
    }
}

#[test]
fn round_trip_arrays_strings_and_safe_rejection() {
    let _kernel = median_mock::lock();
    {
        let d = Dict::new().unwrap();
        assert!(d.keys().unwrap().is_empty());
        d.insert(sym!("i"), 42i64).unwrap();
        d.insert(sym!("f"), 1.25).unwrap();
        d.insert(sym!("s"), sym!("symbol")).unwrap();
        d.insert(sym!("text"), "plain %s text").unwrap();
        let atoms = [Atom::from(3i64), Atom::from(2.5), Atom::from(sym!("value"))];
        d.insert(sym!("atoms"), atoms.as_slice()).unwrap();
        assert_eq!(int(&d, sym!("i")), 42);
        assert!(matches!(d.get(sym!("f")).unwrap(), Some(DictValue::Float(v)) if v == 1.25));
        assert!(
            matches!(d.get(sym!("s")).unwrap(), Some(DictValue::Symbol(v)) if v == sym!("symbol"))
        );
        for _ in 0..2 {
            assert!(
                matches!(d.get(sym!("text")).unwrap(), Some(DictValue::Str(v)) if v == c"plain %s text")
            );
        }
        // The mock models getatom's in-place string conversion; this proves reads avoided it.
        assert_eq!(
            unsafe { max_sys::dictionary_entryisstring(d.as_ptr(), sym!("text").as_ptr()) },
            1
        );
        let Some(DictValue::Atoms(values)) = d.get(sym!("atoms")).unwrap() else {
            panic!()
        };
        assert_eq!(values[0].get_int(), 3);
        assert_eq!(values[1].get_float(), 2.5);
        assert_eq!(values[2].get_symbol(), sym!("value"));
        let mut a = AtomArray::new(&[]).unwrap();
        a.push(9i64.into()).unwrap();
        a.extend(&atoms).unwrap();
        assert_eq!(a.len(), 4);
        let mut copy = a.try_clone().unwrap();
        copy.clear();
        assert!(copy.is_empty());
        assert_eq!(a.len(), 4);
        d.insert(sym!("builder"), a).unwrap();
        let mut object = Atom::from(0i64);
        object.set_obj(std::ptr::null_mut());
        assert_eq!(
            d.insert(sym!("bad"), std::slice::from_ref(&object)),
            Err(DictError::ObjectAtom)
        );
        assert!(matches!(
            AtomArray::new(&[object]),
            Err(DictError::ObjectAtom)
        ));
        assert!(!d.contains_key(sym!("bad")));
        assert_eq!(
            d.keys().unwrap(),
            [
                sym!("i"),
                sym!("f"),
                sym!("s"),
                sym!("text"),
                sym!("atoms"),
                sym!("builder")
            ]
        );
        assert!(
            matches!(d.remove(sym!("text")).unwrap(), Some(DictValue::Str(v)) if v == c"plain %s text")
        );
        assert_eq!(d.len(), 5);
        assert!(!d.delete(sym!("absent")).unwrap());
        d.clear().unwrap();
        assert!(d.is_empty());
    }
    assert_eq!(median_mock::containers::live_count(), 0);
}

#[test]
fn sharing_transfer_copy_replace_and_removal_preserve_references() {
    let _kernel = median_mock::lock();
    let sub = Dict::new().unwrap();
    sub.insert(sym!("x"), 1).unwrap();
    {
        let parent = Dict::new().unwrap();
        parent.insert_shared(sym!("child"), &sub).unwrap();
        assert!(child(&parent).ptr_eq(&sub));
    }
    sub.insert(sym!("x"), 2).unwrap();
    let parent = Dict::new().unwrap();
    parent.insert(sym!("child"), &sub).unwrap();
    let copied = child(&parent);
    sub.insert(sym!("x"), 3).unwrap();
    assert_eq!(int(&copied, sym!("x")), 2);
    let transferred = Dict::new().unwrap();
    let address = transferred.as_ptr();
    parent.insert(sym!("child"), transferred).unwrap();
    let retained = child(&parent);
    assert_eq!(retained.as_ptr(), address);
    assert_eq!(int(&copied, sym!("x")), 2);
    let Some(DictValue::Dict(removed)) = parent.remove(sym!("child")).unwrap() else {
        panic!()
    };
    assert!(removed.ptr_eq(&retained));
    assert!(parent.is_empty());
    drop(parent);
    drop(retained);
    removed.insert(sym!("x"), 4).unwrap();
    drop(removed);
    drop(copied);
    drop(sub);
    assert_eq!(median_mock::containers::live_count(), 0);
}

#[test]
fn registry_collision_and_lifetime() {
    let _kernel = median_mock::lock();
    let (d, name) = Dict::new().unwrap().register(None).unwrap();
    d.insert(sym!("x"), 12).unwrap();
    let a = Dict::find_registered(name).unwrap();
    let b = Dict::find_registered(name).unwrap();
    let (existing, returned_name) = Dict::new().unwrap().register(Some(name)).unwrap();
    assert!(existing.ptr_eq(&d));
    assert_eq!(name, returned_name);
    assert_eq!(median_mock::containers::live_count(), 1);
    assert!(matches!(
        d.clone().register(Some(name)),
        Err(RegisterError::AlreadyRegistered(_))
    ));
    let detached = Dict::find_registered_clone(name).unwrap().unwrap();
    assert!(detached.name().is_none());
    assert_eq!(int(&detached, sym!("x")), 12);
    drop(d);
    drop(a);
    drop(existing);
    assert!(Dict::find_registered(name).is_some());
    drop(b);
    assert!(Dict::find_registered(name).is_none());
    drop(detached);
    let bad = Dict::new()
        .unwrap()
        .register(Some(sym!("not a single name")));
    assert!(matches!(bad, Err(RegisterError::InvalidName(_))));
    drop(bad);
    assert_eq!(median_mock::containers::live_count(), 0);
}

#[test]
fn cycle_checks_include_transfers_and_concurrent_opposite_inserts() {
    let _kernel = median_mock::lock();
    let a = Dict::new().unwrap();
    let b = Dict::new().unwrap();
    assert_eq!(a.insert_shared(sym!("child"), &a), Err(DictError::Cycle));
    assert_eq!(a.insert(sym!("child"), a.clone()), Err(DictError::Cycle));
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|s| {
        let one = s.spawn(|| {
            barrier.wait();
            a.insert_shared(sym!("child"), &b)
        });
        let two = s.spawn(|| {
            barrier.wait();
            b.insert_shared(sym!("child"), &a)
        });
        let results = [one.join().unwrap(), two.join().unwrap()];
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| **r == Err(DictError::Cycle))
                .count(),
            1
        );
    });
    drop(a);
    drop(b);
    assert_eq!(median_mock::containers::live_count(), 0);
}

#[test]
fn concurrent_replace_and_read_uses_transaction_lock() {
    let _kernel = median_mock::lock();
    let d = Dict::new().unwrap();
    std::thread::scope(|s| {
        for writer in 0..2 {
            let d = &d;
            s.spawn(move || {
                for i in 0..10_000 {
                    d.insert(sym!("text"), format!("{writer}:{i}").as_str())
                        .unwrap();
                    let child = Dict::new().unwrap();
                    child.insert(sym!("x"), i).unwrap();
                    d.insert(sym!("child"), child).unwrap();
                }
            });
        }
        s.spawn(|| {
            for _ in 0..10_000 {
                if let Some(DictValue::Str(text)) = d.get(sym!("text")).unwrap() {
                    assert!(text.to_str().unwrap().contains(':'));
                }
                if let Some(DictValue::Dict(c)) = d.get(sym!("child")).unwrap() {
                    assert!((0..10_000).contains(&int(&c, sym!("x"))));
                }
            }
        });
    });
    assert!(matches!(
        d.get_path("child::x").unwrap(),
        Some(DictValue::Int(_))
    ));
    assert!(matches!(d.get_path("::child"), Err(DictError::InvalidPath)));
    assert!(matches!(d.get_path("child::"), Err(DictError::InvalidPath)));
    drop(d);
    assert_eq!(median_mock::containers::live_count(), 0);
}

#[test]
fn transactions_unlock_on_panic_and_copies_do_not_share_children() {
    let _kernel = median_mock::lock();
    let d = Dict::new().unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        d.transaction(|d| {
            d.insert(sym!("x"), 10).unwrap();
            let child = Dict::new().unwrap();
            child.insert(sym!("x"), 20).unwrap();
            d.insert(sym!("child"), child).unwrap();
            panic!("abort the transaction closure");
        });
    }));
    assert!(panic.is_err());
    // A transaction gives exclusion, not rollback, and must release the recursive lock.
    std::thread::scope(|s| {
        s.spawn(|| {
            assert_eq!(int(&d, sym!("x")), 10);
        })
        .join()
        .unwrap();
    });
    let other = Dict::new().unwrap();
    other.merge_from(&d).unwrap();
    child(&d).insert(sym!("x"), 30).unwrap();
    assert_eq!(int(&child(&other), sym!("x")), 20);
    other.insert(sym!("x"), 99).unwrap();
    other.copy_unique_from(&d).unwrap();
    assert_eq!(int(&other, sym!("x")), 99);
    // Opposite copies must not hold a source lock while waiting for a destination lock.
    std::thread::scope(|s| {
        s.spawn(|| {
            for _ in 0..100 {
                d.merge_from(&other).unwrap();
            }
        });
        s.spawn(|| {
            for _ in 0..100 {
                other.merge_from(&d).unwrap();
            }
        });
    });
    #[cfg(debug_assertions)]
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || d.transaction(|_| other.len())
        ))
        .is_err()
    );
    drop(other);
    drop(d);
    assert_eq!(median_mock::containers::live_count(), 0);
}
