//! A port of the SDK's `misc/master`: sends every message it receives to the `median.servant`
//! objects of the same name, through a registered conduit object that the masters of that name
//! share.
//!
//! The conduit is the NOBOX class `median.conduit`, registered in the `median.conduit` namespace
//! (the SDK uses `conduit` for both), so the port and the C objects do not see each other. The
//! masters sharing a conduit are counted in this external instead of with `retain` and `release`
//! messages to the conduit. The SDK's `notify` method of `master` does nothing and is left out.

use std::{cell::RefCell, ptr::NonNull, sync::Mutex};

use median::{atom::AtomValue, dict::AtomArray, notify::Registration, object::ObjBox, prelude::*};

fn conduit() -> SymbolRef {
    median::sym!("median.conduit")
}

#[derive(MaxObj)]
#[max(name = "median.conduit", class_type = nobox, no_methods)]
pub struct Conduit {
    _registration: Mutex<Registration>,
}

impl MaxObjWrapped for Conduit {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let name: SymbolRef = builder.args().at(0).unwrap_or_default()?;
        if name.is_empty() {
            return Err("a conduit needs a name".into());
        }
        let registration = builder
            .try_register(conduit(), name)
            .map_err(|_| format!("could not register the conduit {name}"))?;
        Ok(Self {
            _registration: Mutex::new(registration),
        })
    }
}

struct Shared {
    name: SymbolRef,
    masters: usize,
    conduit: ObjBox<MaxObjWrapper<Conduit>>,
}

thread_local! {
    // Masters are made and freed on the main thread, so only its copy is used.
    static CONDUITS: RefCell<Vec<Shared>> = const { RefCell::new(Vec::new()) };
}

#[derive(MaxObj)]
#[max(name = "median.master")]
pub struct Master {
    name: SymbolRef,
    // `None` for an empty name, as in the SDK.
    conduit: Option<ObjRef>,
}

impl MaxObjWrapped for Master {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let name: SymbolRef = builder.args().at(0).unwrap_or_default()?;
        builder.inlet(Inlet::default().assist("Messages for my sweet servant"));
        Ok(Self {
            name,
            conduit: (!name.is_empty()).then(|| retain(name)),
        })
    }
}

fn retain(name: SymbolRef) -> ObjRef {
    let counted = CONDUITS.with_borrow_mut(|list| {
        let shared = list.iter_mut().find(|s| s.name == name)?;
        shared.masters += 1;
        Some(NonNull::from(&*shared.conduit))
    });
    let ptr = counted.unwrap_or_else(|| {
        // Made outside the borrow: registering it notifies the waiting servants.
        let conduit = MaxObjWrapper::<Conduit>::new(conduit(), &[name.into()]);
        let ptr = NonNull::from(&*conduit);
        CONDUITS.with_borrow_mut(|list| {
            list.push(Shared {
                name,
                masters: 1,
                conduit,
            })
        });
        ptr
    });
    // SAFETY: the conduit lives until `release` takes back the count added above, which
    // `Master::drop` does after the last use of the handle.
    unsafe { ObjRef::from_raw(ptr.cast()) }
}

fn release(name: SymbolRef) {
    let freed = CONDUITS.with_borrow_mut(|list| {
        let i = list.iter().position(|s| s.name == name)?;
        list[i].masters -= 1;
        (list[i].masters == 0).then(|| list.swap_remove(i))
    });
    // Freed outside the borrow: unregistering notifies the servants.
    drop(freed);
}

impl Drop for Master {
    fn drop(&mut self) {
        if self.conduit.is_some() {
            release(self.name);
        }
    }
}

#[median::methods]
impl Master {
    #[bang]
    pub fn bang(&self) {
        self.anything(&median::sym!("bang"), &[]);
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.anything(&median::sym!("int"), &[n.into()]);
    }

    #[float]
    pub fn float(&self, f: f64) {
        self.anything(&median::sym!("float"), &[f.into()]);
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        self.anything(&median::sym!("list"), atoms);
    }

    #[any]
    pub fn anything(&self, sel: &SymbolRef, atoms: &[Atom]) {
        let Some(conduit) = &self.conduit else {
            return;
        };
        let mut message = vec![Atom::from(*sel)];
        message.extend(atoms.iter().map(copy_atom));
        let Ok(array) = AtomArray::new(&message) else {
            return;
        };
        let array = array.into_raw();
        // GAP: object_notify with data
        // SAFETY: the conduit is alive while this master holds it, and the atomarray is alive
        // until it is freed after the notification, which the servants handle synchronously.
        unsafe {
            median::max_sys::object_notify(
                conduit.as_ptr().cast(),
                median::sym!("sendmessage").as_ptr(),
                array.cast(),
            );
            median::max_sys::object_free(array.cast());
        }
    }
}

fn copy_atom(a: &Atom) -> Atom {
    match a.get_value() {
        Some(AtomValue::Int(i)) => i.into(),
        Some(AtomValue::Float(f)) => f.into(),
        _ => a.get_symbol().into(),
    }
}

median::ext_main! { Master, Conduit }

#[cfg(test)]
mod tests {
    use median::{atom::AtomValue, notify::Subscription, prelude::*};
    use median_mock::Atom as MockAtom;
    use std::sync::Mutex;

    // Stands in for `median.servant`, which is another external: outputs `sendmessage` as the
    // message it carries and any other notification as `note <message>`.
    #[derive(MaxObj)]
    #[max(name = "test.listener")]
    pub struct Listener {
        _subscription: Mutex<Subscription>,
        out: Outlet<Any>,
    }

    impl MaxObjWrapped for Listener {
        fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
            let name: SymbolRef = builder.args().at(0).get()?.unwrap();
            let subscription = builder.subscribe(super::conduit(), name, Some(super::conduit()));
            Ok(Self {
                _subscription: Mutex::new(subscription),
                out: builder.outlet(Outlet::any()),
            })
        }
    }

    #[median::methods]
    impl Listener {
        #[notify]
        pub fn notify(&self, n: &Notification) {
            if *n.message() != median::sym!("sendmessage") {
                let _ = self
                    .out
                    .send(&median::sym!("note"), &[(*n.message()).into()]);
                return;
            }
            // SAFETY: only a conduit notifies this object, and `sendmessage` carries a live
            // atomarray.
            let atoms = unsafe { Atom::from_object_raw(n.data().cast()).as_atomarray_unchecked() };
            let atoms = atoms.unwrap().unwrap();
            let (sel, args) = atoms.split_first().unwrap();
            let Some(AtomValue::Symbol(sel)) = sel.get_value() else {
                panic!("the selector is a symbol");
            };
            let _ = self.out.send(&sel, args);
        }
    }

    fn outputs(k: &median_mock::Kernel) -> Vec<(String, Vec<MockAtom>)> {
        k.take_outlet_log()
            .into_iter()
            .map(|e| (e.selector, e.atoms))
            .collect()
    }

    fn note(msg: &str) -> (String, Vec<MockAtom>) {
        ("note".into(), vec![MockAtom::Sym(msg.into())])
    }

    #[test]
    fn masters_share_a_conduit() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        Listener::register();
        let l = k.new_object("test.listener", &[MockAtom::Sym("bus".into())]);
        let objects = k.live_object_count();
        let a = k.new_object("median.master", &[MockAtom::Sym("bus".into())]);
        assert!(!a.is_null());
        assert_eq!(outputs(&k), [note("globalsymbol_binding")]);
        let b = k.new_object("median.master", &[MockAtom::Sym("bus".into())]);
        // The two masters and one conduit.
        assert_eq!(k.live_object_count(), objects + 3);
        k.send_int(a, 5).unwrap();
        k.send(
            b,
            "hello",
            &[MockAtom::Float(1.5), MockAtom::Sym("x".into())],
        )
        .unwrap();
        k.send_bang(b).unwrap();
        k.send_list(a, &[MockAtom::Long(1), MockAtom::Long(2)])
            .unwrap();
        assert_eq!(
            outputs(&k),
            [
                ("int".to_string(), vec![MockAtom::Long(5)]),
                (
                    "hello".to_string(),
                    vec![MockAtom::Float(1.5), MockAtom::Sym("x".into())]
                ),
                ("bang".to_string(), vec![]),
                (
                    "list".to_string(),
                    vec![MockAtom::Long(1), MockAtom::Long(2)]
                ),
            ]
        );
        assert!(k.free_object(a));
        k.send_float(b, 0.25).unwrap();
        assert_eq!(
            outputs(&k),
            [("float".to_string(), vec![MockAtom::Float(0.25)])]
        );
        assert!(k.free_object(b));
        assert_eq!(outputs(&k), [note("globalsymbol_unbinding")]);
        assert_eq!(k.live_object_count(), objects);
        assert!(k.free_object(l));
    }

    #[test]
    fn unnamed_master_sends_nowhere() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let objects = k.live_object_count();
        let x = k.new_object("median.master", &[]);
        assert!(!x.is_null());
        assert_eq!(k.live_object_count(), objects + 1);
        k.send_int(x, 1).unwrap();
        assert!(k.take_outlet_log().is_empty());
        assert_eq!(
            k.assist(x, false, 0).as_deref(),
            Some("Messages for my sweet servant")
        );
        assert!(k.free_object(x));
    }
}
