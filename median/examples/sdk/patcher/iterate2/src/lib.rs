//! A port of the SDK's `patcher/iterate2`: on bang, posts the patcher name and rect of every box
//! in its patcher and the subpatchers; `open_subpatcher <name>` brings every patcher of that name
//! holding a box to the front.
//!
//! The C keeps the name in the object for its iteration callback; the port passes it to the
//! callback instead. Finding the patcher and iterating it have no median API; median-mock has no
//! patcher, so the tests cover only an object outside one.

use std::{
    borrow::Cow,
    ffi::{c_long, c_void},
    ptr::null_mut,
};

use median::{max_sys, prelude::*};

// GAP: PI_DEEP and PI_WANTBOX (`ext_maxtypes.h`) are not in max_sys.
const PI_DEEP: c_long = 1;
const PI_WANTBOX: c_long = 4;

type IterateFn = unsafe extern "C" fn(*mut c_void, *mut max_sys::t_object) -> c_long;

#[derive(MaxObj)]
#[max(name = "median.iterate2")]
pub struct Iterate2 {
    obj: ObjRef,
}

impl MaxObjWrapped for Iterate2 {
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        post!("I am the iterate2 object");
    }

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
impl Iterate2 {
    #[bang]
    pub fn bang(&self) {
        // SAFETY: `post_box` takes the `ObjRef` it is given.
        unsafe { self.iterate(post_box, &self.obj as *const ObjRef as *mut c_void) };
    }

    #[sel("open_subpatcher")]
    pub fn open_subpatcher(&self, s: SymbolRef) {
        if s.is_empty() {
            return;
        }
        // SAFETY: `open_box_patcher` takes the `SymbolRef` it is given.
        unsafe { self.iterate(open_box_patcher, &s as *const SymbolRef as *mut c_void) };
    }

    /// Calls `f(arg, box)` for every box in the object's patcher and its subpatchers.
    ///
    /// # Safety
    /// `f` must accept `arg`, which is used only during this call.
    unsafe fn iterate(&self, f: IterateFn, arg: *mut c_void) {
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
        let mut result: c_long = 0;
        // SAFETY: the patcher outlives the objects in it; `iterate` calls `f` with `arg` and each
        // box before it returns, and the caller guarantees `f` accepts `arg`.
        // GAP: object_method
        unsafe {
            max_sys::object_method(
                jp.cast(),
                median::sym!("iterate").as_ptr(),
                f,
                arg,
                PI_DEEP | PI_WANTBOX,
                &mut result as *mut c_long,
            )
        };
    }
}

/// Posts the box's patcher name and rect, for `x`.
///
/// # Safety
/// `x` must point to a live `ObjRef` and `b` to a live box.
unsafe extern "C" fn post_box(x: *mut c_void, b: *mut max_sys::t_object) -> c_long {
    let mut r = max_sys::t_rect {
        x: 0.,
        y: 0.,
        width: 0.,
        height: 0.,
    };
    // SAFETY: the caller's contract; a box's patcher outlives it and Max's symbols are interned.
    // GAP: patcher access (jbox_*, jpatcher_*)
    let (obj, name) = unsafe {
        max_sys::jbox_get_patching_rect(b, &mut r);
        let p = max_sys::jbox_get_patcher(b);
        (
            &*(x as *const ObjRef),
            SymbolRef::from_raw(max_sys::jpatcher_get_name(p)),
        )
    };
    median::object_post!(
        obj,
        "in {name}, box @ x {} y {}, w {}, h {}",
        r.x as i64,
        r.y as i64,
        r.width as i64,
        r.height as i64
    );
    0
}

/// Brings the box's patcher to the front if its name is `name`.
///
/// # Safety
/// `name` must point to a live `SymbolRef` and `b` to a live box.
unsafe extern "C" fn open_box_patcher(name: *mut c_void, b: *mut max_sys::t_object) -> c_long {
    // SAFETY: the caller's contract; a box's patcher outlives it and Max's symbols are interned.
    // GAP: patcher access (jbox_*, jpatcher_*), typedmess
    unsafe {
        let p = max_sys::jbox_get_patcher(b);
        if SymbolRef::from_raw(max_sys::jpatcher_get_name(p)) == *(name as *const SymbolRef) {
            max_sys::typedmess(p, median::sym!("front").as_ptr(), 0, null_mut());
        }
    }
    0
}

median::ext_main! { Iterate2 }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn outside_a_patcher_does_nothing() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        assert_eq!(k.take_console_log()[0].text, "I am the iterate2 object");
        let x = k.new_object("median.iterate2", &[]);
        assert!(!x.is_null());
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("I am inlet 0"));
        k.send_bang(x).unwrap();
        k.send(x, "open_subpatcher", &[Atom::Sym("sub".into())])
            .unwrap();
        k.send(x, "open_subpatcher", &[Atom::Sym("".into())])
            .unwrap();
        assert!(k.take_console_log().is_empty());
        assert!(k.free_object(x));
    }
}
