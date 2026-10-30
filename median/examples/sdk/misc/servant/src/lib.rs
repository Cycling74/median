//! A port of the SDK's `misc/servant`: outputs the messages sent to the `median.master` objects of
//! the same name, which reach it as notifications from the `median.conduit` object of that name.
//!
//! It subscribes in the `median.conduit` namespace, as `median.master` registers there (the SDK
//! uses `conduit`). Without a name, the object is not made, as in the SDK, but with an error.

use std::{borrow::Cow, sync::Mutex};

use median::{notify::Subscription, prelude::*};

fn conduit() -> SymbolRef {
    median::sym!("median.conduit")
}

#[derive(MaxObj)]
#[max(name = "median.servant")]
pub struct Servant {
    obj: ObjRef,
    name: SymbolRef,
    _subscription: Mutex<Subscription>,
    out: Outlet<Any>,
}

impl MaxObjWrapped for Servant {
    fn class_setup(class: &mut Class<MaxObjWrapper<Self>>) {
        // GAP: c_noinlet
        // SAFETY: the class is valid and not registered yet.
        unsafe { (*class.inner()).c_noinlet = 1 };
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let name: SymbolRef = builder.args().at(0).unwrap_or_default()?;
        if name.is_empty() {
            return Err("servant needs a name".into());
        }
        Ok(Self {
            obj: builder.obj(),
            name,
            // Attached to the conduit now if it exists, or when it is registered.
            _subscription: Mutex::new(builder.subscribe(conduit(), name, Some(conduit()))),
            out: builder.outlet(Outlet::any()),
        })
    }

    fn assist(&self, iolet: IoLet) -> Option<Cow<'_, str>> {
        Some(match iolet {
            IoLet::Inlet(i) => format!("I am inlet {i}").into(),
            IoLet::Outlet(_) => {
                format!("Messages received from my dark master {}", self.name).into()
            }
        })
    }
}

#[median::methods]
impl Servant {
    #[notify]
    pub fn notify(&self, n: &Notification) {
        self.obj.post(format!("servant_notify: {}", n.message()));
        if *n.message() != median::sym!("sendmessage") {
            return;
        }
        // GAP: Notification::data
        // SAFETY: the subscription's class filter lets only a `median.conduit` notify this object,
        // and its `sendmessage` data is an atomarray that lives for the call.
        let atoms = unsafe { Atom::from_object_raw(n.data().cast()).as_atomarray_unchecked() };
        if let Ok(Some(atoms)) = atoms {
            if let Some((sel, args)) = atoms.split_first() {
                let _ = self.out.send(&sel.get_symbol(), args);
            }
        }
    }
}

median::ext_main! { Servant }

#[cfg(test)]
mod tests {
    use median::{dict::AtomArray, notify::Registration, prelude::*};
    use median_mock::Atom as MockAtom;
    use std::sync::Mutex;

    // Stands in for the conduit of `median.master`, which is another external.
    #[derive(MaxObj)]
    #[max(name = "median.conduit", class_type = nobox, no_methods)]
    pub struct TestConduit {
        _registration: Mutex<Registration>,
    }

    impl MaxObjWrapped for TestConduit {
        fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
            let name: SymbolRef = builder.args().at(0).get()?.unwrap();
            let registration = builder.try_register(super::conduit(), name).ok().unwrap();
            Ok(Self {
                _registration: Mutex::new(registration),
            })
        }
    }

    fn send_message(conduit: *mut std::ffi::c_void, atoms: &[Atom]) {
        let array = AtomArray::new(atoms).unwrap().into_raw();
        // SAFETY: `conduit` is a live registered object and `array` a live atomarray.
        unsafe {
            median::max_sys::object_notify(
                conduit,
                median::sym!("sendmessage").as_ptr(),
                array.cast(),
            );
            median::max_sys::object_free(array.cast());
        }
    }

    fn console(k: &median_mock::Kernel) -> Vec<String> {
        k.take_console_log().into_iter().map(|e| e.text).collect()
    }

    #[test]
    fn outputs_what_the_conduit_sends() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        TestConduit::register();
        let x = k.new_object("median.servant", &[MockAtom::Sym("bus".into())]);
        assert!(!x.is_null());
        let c = k.new_object("median.conduit", &[MockAtom::Sym("bus".into())]);
        assert_eq!(console(&k), ["servant_notify: globalsymbol_binding"]);
        send_message(
            c,
            &[
                median::sym!("hello").into(),
                Atom::from(1 as t_atom_long),
                median::sym!("x").into(),
            ],
        );
        send_message(c, &[median::sym!("bang").into()]);
        assert_eq!(console(&k).len(), 2);
        let out: Vec<_> = k
            .take_outlet_log()
            .into_iter()
            .map(|e| (e.selector, e.atoms))
            .collect();
        assert_eq!(
            out,
            [
                (
                    "hello".to_string(),
                    vec![MockAtom::Long(1), MockAtom::Sym("x".into())]
                ),
                ("bang".to_string(), vec![]),
            ]
        );
        assert!(k.free_object(c));
        assert_eq!(console(&k), ["servant_notify: globalsymbol_unbinding"]);
        assert_eq!(
            k.assist(x, true, 0).as_deref(),
            Some("Messages received from my dark master bus")
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn needs_a_name() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        assert!(k.new_object("median.servant", &[]).is_null());
        assert!(!k.take_console_log().is_empty());
    }
}
