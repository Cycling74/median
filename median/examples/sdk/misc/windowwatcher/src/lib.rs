//! A port of the SDK's `misc/windowwatcher`: reports changes to the size and scroll position of
//! its patcher's window.
//!
//! Deviation: the C also adds a `boxscreenrectchanged` method to its box with `object_addmethod`,
//! which finds the object again from the box's first inlet. Max does not call such instance
//! methods (the C says so), and getting the object back from a box pointer is the pointer
//! recovery median avoids, so that output is left out. Finding the patcher and its view and
//! attaching to the view have no median API; median-mock has none of them, so the tests cover
//! only an object outside a patcher.

use std::{
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
};

use median::{max_sys, prelude::*};

#[derive(MaxObj)]
#[max(name = "median.windowwatcher")]
pub struct WindowWatcher {
    obj: ObjRef,
    // The patcher holding the object, which outlives it; null outside a patcher.
    patcher: AtomicPtr<max_sys::t_object>,
    // The patcherview attached to: null until attached, and again once it is freed (its `free`
    // notification), so it is never used after Max frees it.
    view: AtomicPtr<max_sys::t_object>,
    // The patcherview does not exist yet while a patcher is being loaded: attach later.
    _attach: Queue,
    out: Outlet<Any>,
}

impl MaxObjWrapped for WindowWatcher {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let obj = builder.obj();
        let mut patcher = null_mut();
        // SAFETY: the object is live and `patcher` is writable; it stays null if there is none.
        // GAP: object_obex_lookup
        unsafe {
            max_sys::object_obex_lookup(
                obj.as_ptr().cast(),
                median::sym!("#P").as_ptr(),
                &mut patcher,
            )
        };
        let attach = builder.queue(Self::attach);
        attach.set();
        Ok(Self {
            obj,
            patcher: AtomicPtr::new(patcher),
            view: AtomicPtr::new(null_mut()),
            _attach: attach,
            out: builder.outlet(Outlet::any()),
        })
    }
}

#[median::methods]
impl WindowWatcher {
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
        if err == 0 {
            self.view.store(view, Ordering::Relaxed);
        }
    }

    #[notify]
    pub fn notify(&self, n: &Notification) {
        let view = self.view.load(Ordering::Relaxed);
        if view.is_null() || n.sender() != view.cast() {
            return;
        }
        if *n.message() == median::sym!("attr_modified") {
            // SAFETY: for `attr_modified` Max passes the attribute object as the data, and an
            // attribute answers `getname` with its name.
            // GAP: object_method
            let name: SymbolRef = unsafe {
                SymbolRef::from_raw(
                    max_sys::object_method(n.data(), median::sym!("getname").as_ptr()).cast(),
                )
            };
            let out = if name == median::sym!("rect") {
                median::sym!("patcherview")
            } else if name == median::sym!("visiblecanvasrect") {
                name
            } else {
                return;
            };
            let mut r = max_sys::t_rect {
                x: 0.,
                y: 0.,
                width: 0.,
                height: 0.,
            };
            // SAFETY: `view` is live until its `free` notification, and `r` is writable.
            // GAP: object_attr_get_rect
            unsafe { max_sys::object_attr_get_rect(view, name.as_ptr(), &mut r) };
            let _ = self
                .out
                .send(&out, &[r.x, r.y, r.width, r.height].map(Atom::from));
        } else if *n.message() == median::sym!("free") {
            self.detach();
        }
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

impl Drop for WindowWatcher {
    fn drop(&mut self) {
        self.detach();
    }
}

median::ext_main! { WindowWatcher }

#[cfg(test)]
mod tests {
    #[test]
    fn outside_a_patcher_watches_nothing() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.windowwatcher", &[]);
        assert!(!x.is_null());
        assert_eq!(k.run_main_queue(), 1);
        assert!(k.take_outlet_log().is_empty());
        assert!(k.take_console_log().is_empty());
        assert!(k.free_object(x));
        assert_eq!(k.live_qelem_count(), 0);
    }
}
