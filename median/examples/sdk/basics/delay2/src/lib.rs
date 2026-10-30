//! A port of the SDK's `basics/delay2`: an ITM-based `delay`. bang schedules a bang out the left
//! outlet after the `delaytime` attribute (a time value in any ITM unit, quantized by the
//! `quantize` attribute) and one out the right outlet from an ordinary clock after the same time
//! in milliseconds; stop cancels both. A number or time value sets the delay, and in the left
//! inlet also schedules.
//!
//! Untested: `median-mock` has none of the ITM functions (`class_time_addattr`, `time_*`,
//! `itm_*`), so the class cannot even be registered there.

use std::{
    ffi::{CStr, c_long, c_void},
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
};

use median::{max_sys, prelude::*};

// `ext_itm.h` time object flags, which the bindings lack.
const TIME_FLAGS_TICKSONLY: c_long = 0x0002;
const TIME_FLAGS_USECLOCK: c_long = 0x0010;
const TIME_FLAGS_TRANSPORT: c_long = 0x0100;

#[derive(MaxObj)]
#[max(name = "median.delay2")]
pub struct Delay2 {
    obj: ObjRef,
    // Owned `t_timeobject`s, freed on drop.
    delaytime: AtomicPtr<max_sys::t_object>,
    quantize: AtomicPtr<max_sys::t_object>,
    clock: Clock,
    out: Outlet<Bang>,
    clock_out: Outlet<Bang>,
}

fn sym(s: &CStr) -> *mut max_sys::t_symbol {
    SymbolRef::from(s).as_ptr()
}

impl MaxObjWrapped for Delay2 {
    fn class_setup(class: &mut Class<MaxObjWrapper<Self>>) {
        // GAP: ITM time attributes.
        // SAFETY: the class is live and being set up; the names are NUL-terminated.
        unsafe {
            max_sys::class_time_addattr(
                class.inner(),
                c"delaytime".as_ptr(),
                c"Delay Time".as_ptr(),
                TIME_FLAGS_TICKSONLY | TIME_FLAGS_USECLOCK | TIME_FLAGS_TRANSPORT,
            );
            max_sys::class_time_addattr(
                class.inner(),
                c"quantize".as_ptr(),
                c"Quantization".as_ptr(),
                TIME_FLAGS_TICKSONLY,
            );
        }
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("bang Gets Delayed, stop Cancels"));
        builder.inlet(Inlet::proxy().assist("Set Delay Time"));
        let out = builder.outlet(Outlet::bang().assist("Delayed bang"));
        let clock_out = builder.outlet(Outlet::bang().assist("Another Delayed bang"));
        // GAP: ITM time objects.
        // SAFETY: the owner is the object under construction, which outlives its time objects
        // (freed in `drop`); `tick` takes the owner, as a time object's task is called. The
        // task cannot run before `new` returns: nothing is scheduled yet.
        let (delaytime, quantize) = unsafe {
            let owner = builder.max_obj();
            let task = std::mem::transmute::<
                unsafe extern "C" fn(*mut MaxObjWrapper<Delay2>),
                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            >(tick);
            let delaytime: *mut max_sys::t_object = max_sys::time_new(
                owner,
                sym(c"delaytime"),
                Some(task),
                TIME_FLAGS_TICKSONLY | TIME_FLAGS_USECLOCK,
            )
            .cast();
            let quantize: *mut max_sys::t_object =
                max_sys::time_new(owner, sym(c"quantize"), None, TIME_FLAGS_TICKSONLY).cast();
            let zero = Atom::from(0.);
            let initial = builder.args().positional().first().unwrap_or(&zero);
            set_time(delaytime, null_mut(), std::slice::from_ref(initial));
            set_time(quantize, null_mut(), std::slice::from_ref(&zero));
            (delaytime, quantize)
        };
        Ok(Self {
            obj: builder.obj(),
            delaytime: AtomicPtr::new(delaytime),
            quantize: AtomicPtr::new(quantize),
            clock: builder.clock(Self::clock_tick),
            out,
            clock_out,
        })
    }
}

impl Drop for Delay2 {
    fn drop(&mut self) {
        // GAP: ITM time objects.
        // SAFETY: the time objects are ours and nothing uses them after this.
        unsafe {
            max_sys::object_free(self.delaytime.get_mut().cast());
            max_sys::object_free(self.quantize.get_mut().cast());
        }
    }
}

/// # Safety
/// `tx` must be a live time object, `s` null or a symbol.
unsafe fn set_time(tx: *mut max_sys::t_object, s: *mut max_sys::t_symbol, atoms: &[Atom]) {
    // SAFETY: the caller's contract; `Atom` is a transparent `t_atom`, which `time_setvalue`
    // only reads
    unsafe {
        max_sys::time_setvalue(
            tx,
            s,
            atoms.len() as c_long,
            atoms.as_ptr().cast_mut().cast(),
        )
    };
}

/// The `delaytime` task. As in the SDK, a task already dispatched when the object is freed
/// races the free.
///
/// # Safety
/// `x` must be a live `median.delay2` object.
unsafe extern "C" fn tick(x: *mut MaxObjWrapper<Delay2>) {
    // SAFETY: the caller's contract
    let w = unsafe { &*x };
    median::ffi::guard(w, "delaytime", || {
        let _ = w.wrapped().out.send();
    });
}

#[median::methods]
impl Delay2 {
    #[bang]
    pub fn bang(&self) {
        let delaytime = self.delaytime.load(Ordering::Relaxed);
        // GAP: ITM time objects.
        // SAFETY: the time objects live as long as `self`.
        let ms = unsafe {
            max_sys::time_schedule(delaytime, self.quantize.load(Ordering::Relaxed));
            let ticks = max_sys::time_getticks(delaytime);
            max_sys::itm_tickstoms(max_sys::time_getitm(delaytime).cast(), ticks)
        };
        self.clock.fdelay(ms);
    }

    #[sel("stop")]
    pub fn stop(&self) {
        // GAP: ITM time objects.
        // SAFETY: the time object lives as long as `self`.
        unsafe { max_sys::time_stop(self.delaytime.load(Ordering::Relaxed)) };
        self.clock.cancel();
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        self.float(n as f64);
    }

    #[float]
    pub fn float(&self, f: f64) {
        self.set(null_mut(), &[f.into()]);
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        self.set(null_mut(), atoms);
    }

    #[any]
    pub fn anything(&self, sel: &SymbolRef, atoms: &[Atom]) {
        self.set(sel.as_ptr(), atoms);
    }

    fn set(&self, sel: *mut max_sys::t_symbol, atoms: &[Atom]) {
        // GAP: ITM time objects.
        // SAFETY: the time object lives as long as `self`; `sel` is null or a symbol.
        unsafe { set_time(self.delaytime.load(Ordering::Relaxed), sel, atoms) };
        if self.obj.proxy_inlet() == 0 {
            self.bang();
        }
    }

    fn clock_tick(&self) {
        let _ = self.clock_out.send();
    }
}

median::ext_main! { Delay2 }
