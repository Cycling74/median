//! A port of the SDK's `patcher/sheep`: registers itself under a unique name and adds itself to a
//! `sheephash` hashtab kept in its patcher, where `median.shepherd` objects find it; `baah` sends
//! a `sheep` notification, and the `A_CANT` method `shepherd_moved` posts.
//!
//! The hashtab is Max's (its `hashtab_entry_new`/`hashtab_entry_free` notifications reach the
//! shepherds), so it stays raw. Finding the patcher, its obex, the hashtab, `symbol_unique` and
//! the `A_CANT` method have no median API; median-mock has no patcher, so the tests cover only
//! an object outside one (unregistered, so `baah` reaches no one) and `shepherd_moved` called
//! directly.

use std::{borrow::Cow, ptr::null_mut, sync::Mutex};

use median::{max_sys, notify::Registration, prelude::*};

#[derive(MaxObj)]
#[max(name = "median.sheep")]
pub struct Sheep {
    obj: ObjRef,
    // The key the sheep is stored under in `sheephash`; `None` outside a patcher.
    name: Option<SymbolRef>,
    _registration: Mutex<Option<Registration>>,
}

impl MaxObjWrapped for Sheep {
    fn class_setup(class: &mut Class<MaxObjWrapper<Self>>) {
        // GAP: A_CANT method. median registers only its own fixed set of `A_CANT` methods.
        // SAFETY: `shepherd_moved` takes the object and nothing else, as an `A_CANT` method with
        // no arguments is called; the class is the live class being set up.
        unsafe {
            let m = std::mem::transmute::<
                unsafe extern "C" fn(*mut MaxObjWrapper<Sheep>),
                unsafe extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void,
            >(shepherd_moved);
            max_sys::class_addmethod(
                class.inner(),
                Some(m),
                c"shepherd_moved".as_ptr(),
                max_sys::e_max_atomtypes::A_CANT as std::ffi::c_int,
                0,
            );
        }
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let obj = builder.obj();
        // The obex has no `#P` yet while the object is made; `#P` is bound to the patcher being
        // loaded.
        // SAFETY: `gensym` symbols are live; `s_thing` is null or the bound object.
        let jp = unsafe { (*median::sym!("#P").as_ptr()).s_thing };
        let mut name = None;
        let mut registration = None;
        if !jp.is_null() {
            // SAFETY: the patcher outlives the objects in it.
            let ht = unsafe { sheephash(jp, true) };
            // GAP: symbol_unique
            // SAFETY: `symbol_unique` returns an interned symbol.
            let unique = unsafe { SymbolRef::from_raw(max_sys::symbol_unique()) };
            if let Ok(r) = builder.try_register(median::sym!("box"), unique) {
                // The hashtab only refers to the sheep (`OBJ_FLAG_REF`), which removes
                // itself on drop.
                // SAFETY: `ht` is live and the object is registered under `unique`.
                // GAP: hashtab (shared with median.shepherd through the patcher's obex)
                unsafe {
                    max_sys::hashtab_storeflags(
                        ht,
                        unique.as_ptr(),
                        obj.as_ptr(),
                        max_sys::e_max_datastore_flags::OBJ_FLAG_REF as _,
                    )
                };
                name = Some(unique);
                registration = Some(r);
            }
        }
        Ok(Self {
            obj,
            name,
            _registration: Mutex::new(registration),
        })
    }

    fn assist(&self, iolet: IoLet) -> Option<Cow<'_, str>> {
        Some(match iolet {
            IoLet::Inlet(i) => format!("I am inlet {i}").into(),
            IoLet::Outlet(i) => format!("I am outlet {i}").into(),
        })
    }
}

/// The `sheephash` hashtab in the obex of patcher `jp`, made and stored there if `create`, else
/// null if there is none.
///
/// # Safety
/// `jp` must be a live patcher whose `sheephash`, if any, is a hashtab.
unsafe fn sheephash(jp: *mut max_sys::t_object, create: bool) -> *mut max_sys::t_hashtab {
    let key = median::sym!("sheephash");
    let mut ht = null_mut();
    // SAFETY: the caller's contract; objects in the obex are freed with the patcher.
    // GAP: object_obex_lookup, object_obex_store, hashtab
    unsafe {
        max_sys::object_obex_lookup(jp.cast(), key.as_ptr(), &mut ht);
        if ht.is_null() && create {
            ht = max_sys::hashtab_new(0).cast();
            max_sys::object_obex_store(jp.cast(), key.as_ptr(), ht);
        }
    }
    ht.cast()
}

/// # Safety
/// `x` must be a live `median.sheep` object.
unsafe extern "C" fn shepherd_moved(x: *mut MaxObjWrapper<Sheep>) {
    // SAFETY: the caller's contract
    let w = unsafe { &*x };
    median::ffi::guard(w, "shepherd_moved", || {
        w.wrapped().obj.post("Oh, no! The shepherd!!!")
    });
}

#[median::methods]
impl Sheep {
    #[sel("baah")]
    pub fn baah(&self) {
        let _ = self.obj.notify(median::sym!("sheep"));
    }
}

impl Drop for Sheep {
    fn drop(&mut self) {
        let Some(name) = self.name else { return };
        let mut jp = null_mut();
        // SAFETY: the object is live and `jp` is writable; it stays null if there is no patcher.
        // GAP: object_obex_lookup
        unsafe {
            max_sys::object_obex_lookup(
                self.obj.as_ptr().cast(),
                median::sym!("#P").as_ptr(),
                &mut jp,
            )
        };
        if jp.is_null() {
            return;
        }
        // SAFETY: the patcher outlives the objects in it; its `sheephash` is a hashtab.
        let ht = unsafe { sheephash(jp, false) };
        if !ht.is_null() {
            // SAFETY: `ht` is live.
            // GAP: hashtab
            unsafe { max_sys::hashtab_chuckkey(ht, name.as_ptr()) };
        }
    }
}

median::ext_main! { Sheep }

#[cfg(test)]
mod tests {
    use std::ffi::c_void;

    #[test]
    fn outside_a_patcher() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.sheep", &[]);
        assert!(!x.is_null());
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("I am inlet 0"));
        k.send(x, "baah", &[]).unwrap();
        assert!(k.send(x, "shepherd_moved", &[]).is_err());
        let (f, types) = k.method(x, "shepherd_moved").unwrap();
        assert_eq!(types, [median::max_sys::e_max_atomtypes::A_CANT as i32]);
        // SAFETY: `shepherd_moved` is `A_CANT` with no arguments: it takes the object only.
        unsafe {
            let f: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(f);
            f(x);
        }
        let console: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(console, ["Oh, no! The shepherd!!!"]);
        assert!(k.free_object(x));
    }
}
