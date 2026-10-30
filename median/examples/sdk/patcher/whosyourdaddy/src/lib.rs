//! A port of the SDK's `patcher/whosyourdaddy`: on bang, posts what holds its patcher (a
//! bpatcher, an abstraction or a subpatcher box), or the class of the object a patcher outside a
//! box belongs to.
//!
//! Finding the patcher and every call on it have no median API; median-mock has no patcher, so
//! the tests cover only an object outside one.

use std::{borrow::Cow, ffi::c_void, ptr::null_mut};

use median::{max_sys, prelude::*};

#[derive(MaxObj)]
#[max(name = "median.whosyourdaddy")]
pub struct WhosYourDaddy {
    obj: ObjRef,
}

impl MaxObjWrapped for WhosYourDaddy {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { obj: builder.obj() })
    }

    fn assist(&self, iolet: IoLet) -> Option<Cow<'_, str>> {
        Some(match iolet {
            IoLet::Inlet(i) => format!("I am inlet {i}").into(),
            IoLet::Outlet(i) => format!("I am outlet {i}").into(),
        })
    }
}

#[median::methods]
impl WhosYourDaddy {
    #[bang]
    pub fn bang(&self) {
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
        if !jp.is_null() {
            // SAFETY: the patcher outlives the objects in it.
            unsafe { report(jp) };
        }
    }
}

/// Posts what holds the patcher `jp`.
///
/// # Safety
/// `jp` must be a live patcher.
unsafe fn report(jp: *mut max_sys::t_object) {
    // GAP: patcher access (jpatcher_*, jbox_*), object_method, object_attr_getsym
    // SAFETY: the caller's contract; a box holding a patcher outlives it, and every symbol Max
    // returns is null or interned.
    unsafe {
        let jbx = max_sys::jpatcher_get_box(jp);
        if !jbx.is_null() {
            let filepath = SymbolRef::from_raw(max_sys::object_attr_getsym(
                jp.cast(),
                median::sym!("filepath").as_ptr(),
            ));
            if SymbolRef::from_raw(max_sys::object_classname(jbx.cast()))
                == median::sym!("bpatcher")
            {
                median::post!("bpatcher");
            } else if !filepath.is_empty() {
                median::post!("abstraction");
            } else {
                median::post!("subpatcher");
            }
            return;
        }
        let mut target: *mut max_sys::t_object = null_mut();
        // `getassoc` writes the object the patcher belongs to, or leaves it null.
        max_sys::object_method(
            jp.cast(),
            median::sym!("getassoc").as_ptr(),
            &mut target as *mut *mut max_sys::t_object,
        );
        if target.is_null() {
            return;
        }
        let mut p2: *mut max_sys::t_object = null_mut();
        if let Some(m) = max_sys::zgetfn(target, median::sym!("parentpatcher").as_ptr()) {
            // `parentpatcher` takes the object and where to write its patcher.
            let m: unsafe extern "C" fn(
                *mut max_sys::t_object,
                *mut *mut max_sys::t_object,
            ) -> *mut c_void = std::mem::transmute(m);
            m(target, &mut p2);
        }
        if p2.is_null() {
            return;
        }
        let mut b = max_sys::jpatcher_get_firstobject(p2);
        while !b.is_null() {
            let o = max_sys::jbox_get_object(b);
            if o == target {
                median::post!(
                    "{}",
                    SymbolRef::from_raw(max_sys::object_classname(o.cast()))
                );
                return;
            }
            b = max_sys::jbox_get_nextobject(b);
        }
    }
}

median::ext_main! { WhosYourDaddy }

#[cfg(test)]
mod tests {
    #[test]
    fn outside_a_patcher_posts_nothing() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.whosyourdaddy", &[]);
        assert!(!x.is_null());
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("I am inlet 0"));
        k.send_bang(x).unwrap();
        assert!(k.take_console_log().is_empty());
        assert!(k.free_object(x));
    }
}
