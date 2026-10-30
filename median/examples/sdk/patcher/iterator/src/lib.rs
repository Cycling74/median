//! A port of the SDK's `patcher/iterator`: on bang, posts the rect, scripting name and class of
//! every box in its patcher; when the patcher's first view is resized, grows or shrinks its own
//! box by as much.
//!
//! Deviations: the rects are read and written with `object_attr_get_rect`/`object_attr_set_rect`
//! instead of as atoms; the object detaches from the view when the view is freed (the C detaches
//! only in its own `free`, from whatever view is first then). Finding the patcher, its view and
//! its boxes, and attaching to the view, have no median API; median-mock has none of them, so the
//! tests cover only an object outside a patcher.

use std::{
    borrow::Cow,
    ptr::null_mut,
    sync::{
        Mutex,
        atomic::{AtomicPtr, Ordering},
    },
};

use median::{max_sys, prelude::*};

#[derive(MaxObj)]
#[max(name = "median.iterator")]
pub struct PatchIterator {
    obj: ObjRef,
    // The patcher holding the object, which outlives it; null outside a patcher.
    patcher: AtomicPtr<max_sys::t_object>,
    // The patcherview attached to: null until attached, and again once it is freed (its `free`
    // notification), so it is never used after Max frees it.
    view: AtomicPtr<max_sys::t_object>,
    // The view's last width and height.
    cached: Mutex<(f64, f64)>,
    // The patcherview does not exist yet while a patcher is being loaded: attach later.
    _attach: Queue,
}

fn rect() -> max_sys::t_rect {
    max_sys::t_rect {
        x: 0.,
        y: 0.,
        width: 0.,
        height: 0.,
    }
}

impl MaxObjWrapped for PatchIterator {
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        post!("I am the iterator object");
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let obj = builder.obj();
        let patcher = lookup(&obj, median::sym!("#P"));
        let attach = builder.queue(Self::attach);
        attach.set();
        Ok(Self {
            obj,
            patcher: AtomicPtr::new(patcher),
            view: AtomicPtr::new(null_mut()),
            cached: Mutex::new((0., 0.)),
            _attach: attach,
        })
    }

    fn assist(&self, iolet: IoLet) -> Option<Cow<'_, str>> {
        Some(match iolet {
            IoLet::Inlet(i) => format!("I am inlet {i}").into(),
            IoLet::Outlet(i) => format!("I am outlet {i}").into(),
        })
    }
}

/// The object `key` names in `obj`'s obex (`#P` its patcher, `#B` its box), or null.
fn lookup(obj: &ObjRef, key: SymbolRef) -> *mut max_sys::t_object {
    let mut p = null_mut();
    // SAFETY: the object is live and `p` is writable; it stays null if there is no such entry.
    // GAP: object_obex_lookup
    unsafe { max_sys::object_obex_lookup(obj.as_ptr().cast(), key.as_ptr(), &mut p) };
    p
}

#[median::methods]
impl PatchIterator {
    #[bang]
    pub fn bang(&self) {
        let jp = lookup(&self.obj, median::sym!("#P"));
        let mybox = lookup(&self.obj, median::sym!("#B"));
        if jp.is_null() || mybox.is_null() {
            return;
        }
        // SAFETY: the patcher outlives the objects in it, and its boxes are live while it is not
        // edited, which nothing here does; Max's symbols are null or interned.
        // GAP: patcher access (jpatcher_*, jbox_*), object_classname
        unsafe {
            let mut b = max_sys::jpatcher_get_firstobject(jp);
            while !b.is_null() {
                let mut r = rect();
                max_sys::jbox_get_patching_rect(b, &mut r);
                median::object_post!(
                    &self.obj,
                    "found an object at {} {}, w {}, h {}",
                    r.x as i64,
                    r.y as i64,
                    r.width as i64,
                    r.height as i64
                );
                let varname = SymbolRef::from_raw(max_sys::jbox_get_varname(b));
                if !varname.is_empty() {
                    median::object_post!(&self.obj, " it is named {varname}...");
                }
                let o = max_sys::jbox_get_object(b);
                if !o.is_null() {
                    let class = SymbolRef::from_raw(max_sys::object_classname(o.cast()));
                    post!(" it's a(n) {class} object...");
                    if max_sys::jpatcher_is_patcher(o) != 0 {
                        post!(" which is some kind of a patcher. we could recurse here...");
                    }
                }
                if b == mybox {
                    post!(" ...and it's me!");
                }
                b = max_sys::jbox_get_nextobject(b);
            }
        }
    }

    fn attach(&self) {
        let patcher = self.patcher.load(Ordering::Relaxed);
        if patcher.is_null() {
            return;
        }
        // SAFETY: the patcher outlives the objects in it.
        // GAP: object_attr_getobj
        let view = unsafe {
            max_sys::object_attr_getobj(patcher.cast(), median::sym!("firstview").as_ptr())
        };
        if view.is_null() {
            return;
        }
        // SAFETY: the object and the view are live; the attachment is undone in `notify` or `drop`.
        // GAP: object_attach_byptr_register
        let err = unsafe {
            max_sys::object_attach_byptr_register(
                self.obj.as_ptr().cast(),
                view.cast(),
                median::sym!("nobox").as_ptr(),
            )
        };
        if err != 0 {
            return;
        }
        self.view.store(view, Ordering::Relaxed);
        let mut r = rect();
        // SAFETY: the view is live and `r` is writable.
        // GAP: object_attr_get_rect
        if unsafe { max_sys::object_attr_get_rect(view, median::sym!("rect").as_ptr(), &mut r) }
            == 0
        {
            *self.cached.lock().unwrap() = (r.width, r.height);
        }
    }

    #[notify]
    pub fn notify(&self, n: &Notification) {
        let view = self.view.load(Ordering::Relaxed);
        if view.is_null() || n.sender() != view.cast() {
            return;
        }
        if *n.message() == median::sym!("free") {
            self.detach();
            return;
        }
        if *n.message() != median::sym!("attr_modified") {
            return;
        }
        // SAFETY: for `attr_modified` Max passes the attribute object as the data, and an
        // attribute answers `getname` with its name.
        // GAP: object_method
        let name: SymbolRef = unsafe {
            SymbolRef::from_raw(
                max_sys::object_method(n.data(), median::sym!("getname").as_ptr()).cast(),
            )
        };
        post!("'{name}' attribute was modified");
        if name != median::sym!("rect") {
            return;
        }
        let mut vr = rect();
        // SAFETY: `view` is live until its `free` notification, and `vr` is writable.
        // GAP: object_attr_get_rect
        if unsafe { max_sys::object_attr_get_rect(view, name.as_ptr(), &mut vr) } != 0 {
            return;
        }
        // The C reads the rects as longs, so the values are truncated throughout.
        let (w, h) = (vr.width.trunc(), vr.height.trunc());
        post!(
            "new rect: {} {} {} {}",
            vr.x as i64,
            vr.y as i64,
            w as i64,
            h as i64
        );
        let jb = lookup(&self.obj, median::sym!("#B"));
        if jb.is_null() {
            return;
        }
        let mut br = rect();
        // SAFETY: the object's box is live while the object is, and `br` is writable.
        // GAP: object_attr_get_rect
        if unsafe {
            max_sys::object_attr_get_rect(jb, median::sym!("patching_rect").as_ptr(), &mut br)
        } != 0
        {
            return;
        }
        let mut cached = self.cached.lock().unwrap();
        let (dx, dy) = ((w - cached.0).trunc(), (h - cached.1).trunc());
        *cached = (w, h);
        br.width = br.width.trunc() + dx;
        // jnewobj ignores the height.
        br.height = br.height.trunc() + dy;
        // SAFETY: as above.
        // GAP: object_attr_set_rect
        unsafe {
            max_sys::object_attr_set_rect(jb, median::sym!("patching_rect").as_ptr(), &mut br)
        };
    }

    fn detach(&self) {
        let view = self.view.swap(null_mut(), Ordering::Relaxed);
        if !view.is_null() {
            // SAFETY: the object is attached to the view, which is live.
            // GAP: object_detach_byptr
            unsafe { max_sys::object_detach_byptr(self.obj.as_ptr().cast(), view.cast()) };
        }
    }
}

impl Drop for PatchIterator {
    fn drop(&mut self) {
        self.detach();
    }
}

median::ext_main! { PatchIterator }

#[cfg(test)]
mod tests {
    #[test]
    fn outside_a_patcher_does_nothing() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        assert_eq!(k.take_console_log()[0].text, "I am the iterator object");
        let x = k.new_object("median.iterator", &[]);
        assert!(!x.is_null());
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("I am inlet 0"));
        assert_eq!(k.run_main_queue(), 1);
        k.send_bang(x).unwrap();
        assert!(k.take_console_log().is_empty());
        assert!(k.free_object(x));
        assert_eq!(k.live_qelem_count(), 0);
    }
}
