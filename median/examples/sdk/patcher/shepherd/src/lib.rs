//! A port of the SDK's `patcher/shepherd`: attaches to the `sheephash` hashtab in its patcher and
//! to every `median.sheep` in it, posting as sheep come and go and when one sends `sheep`; `boo`
//! calls `shepherd_moved` on every sheep.
//!
//! Deviations: hashtab notifications are acted on only from the shepherd's own hashtab (the C
//! treats any sender of them as a hashtab), and the shepherd forgets the hashtab when it is freed
//! (the C would detach from it after). The hashtab is Max's, for its notifications, so it stays
//! raw. Finding the patcher and its obex, attaching by pointer and the hashtab calls have no
//! median API; median-mock has no patcher, so the tests cover an object outside one and the
//! `sheep` notification.

use std::{
    borrow::Cow,
    ffi::c_void,
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
};

use median::{max_sys, prelude::*};

#[derive(MaxObj)]
#[max(name = "median.shepherd")]
pub struct Shepherd {
    obj: ObjRef,
    // The patcher's `sheephash`, attached to; null outside a patcher and once it is freed.
    ht: AtomicPtr<max_sys::t_hashtab>,
}

impl MaxObjWrapped for Shepherd {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let obj = builder.obj();
        // SAFETY: the object is live.
        let ht = unsafe { attach(&obj) };
        Ok(Self {
            obj,
            ht: AtomicPtr::new(ht),
        })
    }

    fn assist(&self, iolet: IoLet) -> Option<Cow<'_, str>> {
        Some(match iolet {
            IoLet::Inlet(i) => format!("I am inlet {i}").into(),
            IoLet::Outlet(i) => format!("I am outlet {i}").into(),
        })
    }
}

/// Attaches `obj` to its patcher's `sheephash`, made if there is none, and to every sheep in it;
/// returns the hashtab, or null outside a patcher.
///
/// # Safety
/// `obj` must be live, and its patcher's `sheephash`, if any, a hashtab of registered objects.
unsafe fn attach(obj: &ObjRef) -> *mut max_sys::t_hashtab {
    let x = obj.as_ptr();
    let key = median::sym!("sheephash");
    // SAFETY: the caller's contract; the patcher outlives the objects in it and frees what is in
    // its obex.
    // GAP: object_obex_lookup, object_obex_store, object_attach_byptr_register, hashtab
    unsafe {
        let mut jp = null_mut();
        max_sys::object_obex_lookup(x.cast(), median::sym!("#P").as_ptr(), &mut jp);
        if jp.is_null() {
            return null_mut();
        }
        let mut ht = null_mut();
        max_sys::object_obex_lookup(jp.cast(), key.as_ptr(), &mut ht);
        if ht.is_null() {
            ht = max_sys::hashtab_new(0).cast();
            max_sys::object_obex_store(jp.cast(), key.as_ptr(), ht);
        }
        let ht: *mut max_sys::t_hashtab = ht.cast();
        // Registers the hashtab if needed, so it sends `hashtab_entry_new`/`hashtab_entry_free`.
        max_sys::object_attach_byptr_register(x.cast(), ht.cast(), median::sym!("nobox").as_ptr());
        for_each_value(ht, x.cast(), attach_value);
        let n = max_sys::hashtab_getsize(ht);
        if n != 0 {
            median::object_post!(obj, "I, the shepherd, have {n} sheep to tend! What joy!");
        } else {
            obj.post("I, the shepherd, have no sheep to tend. Whatever shall I do?");
        }
        ht
    }
}

/// Calls `f(entry, arg)` for every entry of `ht`.
///
/// # Safety
/// `ht` must be live and `f` must accept `arg`.
unsafe fn for_each_value(
    ht: *mut max_sys::t_hashtab,
    arg: *mut c_void,
    f: unsafe extern "C" fn(*mut max_sys::t_hashtab_entry, *mut c_void),
) {
    // SAFETY: `hashtab_funall` calls `f` with each entry and `arg`; the caller's contract.
    // GAP: hashtab
    unsafe {
        let f = std::mem::transmute::<
            unsafe extern "C" fn(*mut max_sys::t_hashtab_entry, *mut c_void),
            unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        >(f);
        max_sys::hashtab_funall(ht, Some(f), arg);
    }
}

/// # Safety
/// `e` must be a live entry whose value is a registered object, and `x` a live object.
unsafe extern "C" fn attach_value(e: *mut max_sys::t_hashtab_entry, x: *mut c_void) {
    // SAFETY: the caller's contract
    // GAP: object_attach_byptr
    unsafe { max_sys::object_attach_byptr(x, (*e).value.cast()) };
}

/// # Safety
/// `e` must be a live entry, and `x` a live object.
unsafe extern "C" fn detach_value(e: *mut max_sys::t_hashtab_entry, x: *mut c_void) {
    // SAFETY: the caller's contract
    // GAP: object_detach_byptr
    unsafe { max_sys::object_detach_byptr(x, (*e).value.cast()) };
}

#[median::methods]
impl Shepherd {
    #[sel("boo")]
    pub fn boo(&self) {
        let ht = self.ht.load(Ordering::Relaxed);
        if !ht.is_null() {
            // SAFETY: `ht` is live until its `free` notification; its values are sheep, which
            // have `shepherd_moved`.
            // GAP: hashtab
            unsafe { max_sys::hashtab_methodall(ht, median::sym!("shepherd_moved").as_ptr()) };
        }
    }

    #[notify]
    pub fn notify(&self, n: &Notification) {
        let msg = *n.message();
        if msg == median::sym!("sheep") {
            self.obj.post("sheep!!!");
            return;
        }
        let ht = self.ht.load(Ordering::Relaxed);
        if ht.is_null() || n.sender() != ht.cast() {
            return;
        }
        let new = msg == median::sym!("hashtab_entry_new");
        if !new && msg != median::sym!("hashtab_entry_free") {
            if msg == median::sym!("free") {
                self.ht.store(null_mut(), Ordering::Relaxed);
            }
            return;
        }
        let mut obj = null_mut();
        // SAFETY: `ht` is live, and a hashtab passes the entry's key as the data; it sends
        // `hashtab_entry_free` before removing the entry.
        // GAP: hashtab, object_attach_byptr, object_detach_byptr
        let size = unsafe {
            max_sys::hashtab_lookup(ht, n.data().cast(), &mut obj);
            if !obj.is_null() {
                let x = self.obj.as_ptr().cast();
                if new {
                    max_sys::object_attach_byptr(x, obj.cast());
                } else {
                    max_sys::object_detach_byptr(x, obj.cast());
                }
            }
            max_sys::hashtab_getsize(ht)
        };
        if new {
            median::object_post!(&self.obj, "Oh joy! I have {size} sheep to tend.");
        } else if size > 1 {
            median::object_post!(
                &self.obj,
                "Oh untimely death! Only {} sheep left.",
                size - 1
            );
        } else {
            self.obj
                .post("Oh inconsolable sadness! My sheep are gone...");
        }
    }
}

impl Drop for Shepherd {
    fn drop(&mut self) {
        let ht = self.ht.swap(null_mut(), Ordering::Relaxed);
        if ht.is_null() {
            return;
        }
        let x = self.obj.as_ptr().cast();
        // SAFETY: `ht` is live (it has not sent `free`), and the object is attached to it and to
        // its values.
        // GAP: object_detach_byptr
        unsafe {
            for_each_value(ht, x, detach_value);
            max_sys::object_detach_byptr(x, ht.cast());
        }
    }
}

median::ext_main! { Shepherd }

#[cfg(test)]
mod tests {
    use median::{notify::Registration, prelude::*};
    use std::sync::Mutex;

    // Stands in for a `median.sheep`, which is another external.
    #[derive(MaxObj)]
    #[max(name = "median.testsheep", class_type = nobox, no_methods)]
    pub struct TestSheep {
        _registration: Mutex<Registration>,
    }

    impl MaxObjWrapped for TestSheep {
        fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
            let registration = builder
                .try_register(median::sym!("box"), median::sym!("dolly"))
                .ok()
                .unwrap();
            Ok(Self {
                _registration: Mutex::new(registration),
            })
        }
    }

    #[test]
    fn outside_a_patcher_hears_a_sheep() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        TestSheep::register();
        let x = k.new_object("median.shepherd", &[]);
        assert!(!x.is_null());
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("I am inlet 0"));
        k.send(x, "boo", &[]).unwrap();
        assert!(k.take_console_log().is_empty());

        let sheep = k.new_object("median.testsheep", &[]);
        // SAFETY: both objects are live; the shepherd has a `notify` method.
        let p = unsafe {
            median::max_sys::object_attach(
                median::sym!("box").as_ptr(),
                median::sym!("dolly").as_ptr(),
                x,
            )
        };
        assert_eq!(p, sheep);
        // SAFETY: `sheep` is a live registered object.
        unsafe {
            median::max_sys::object_notify(
                sheep,
                median::sym!("sheep").as_ptr(),
                std::ptr::null_mut(),
            )
        };
        // A hashtab notification from anything but its hashtab is ignored.
        // SAFETY: as above.
        unsafe {
            median::max_sys::object_notify(
                sheep,
                median::sym!("hashtab_entry_new").as_ptr(),
                std::ptr::null_mut(),
            )
        };
        let console: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(console, ["sheep!!!"]);
        // SAFETY: as above.
        unsafe {
            median::max_sys::object_detach(
                median::sym!("box").as_ptr(),
                median::sym!("dolly").as_ptr(),
                x,
            )
        };
        assert!(k.free_object(sheep));
        assert!(k.free_object(x));
    }
}
