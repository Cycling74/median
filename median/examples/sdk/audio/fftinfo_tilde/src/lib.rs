//! A port of the SDK's `audio/fftinfo~`: inside a `pfft~` subpatch, outputs its FFT frame size,
//! spectral frame size, hop size and full-spectrum flag when DSP starts and on bang.
//!
//! The SDK object outputs the maximum vector size as the spectral frame size, which median's
//! `dsp64` does not pass on; the port outputs the frame size `pfft~` runs its subpatch at, the
//! FFT size, or half of it without full spectrum. The mock has no `pfft~`: the test stands one in
//! by binding a `t_pfftpub` to `__pfft~__`.

use std::{
    ffi::{c_long, c_short},
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

use median::{max_sys, prelude::*};

// `t_pfftpub` from `r_pfft.h`, which max-sys does not bind. The SDK packs it only on targets
// that set `C74_PRAGMA_STRUCT_PACKPUSH`, none of which median builds for.
#[repr(C)]
struct PfftPub {
    x_obj: max_sys::t_pxobject,
    x_parent: *mut max_sys::t_object,
    x_patcher: *mut max_sys::t_object,
    x_chain: *mut std::ffi::c_void,
    x_fftsize: c_long,
    x_ffthop: c_long,
    x_fftoffset: c_long,
    x_fftindex: c_long,
    x_fullspect: c_short,
}

// From `z_dsp.h`, which max-sys binds without its macros.
const Z_PUT_FIRST: c_short = 4;

// Shared by every object, as in the SDK: the warning is posted once per session.
static WARN: AtomicBool = AtomicBool::new(true);

#[derive(MSPObj)]
#[max(name = "median.fftinfo~")]
pub struct FftinfoTilde {
    obj: ObjRef,
    pfft: AtomicPtr<PfftPub>,
    fftsize: Int64,
    n: Int64,
    ffthop: Int64,
    fullspect: Int64,
    // Left to right: FFT size, spectral frame size, hop, full-spectrum flag.
    outs: [Outlet<Int>; 4],
}

// The `pfft~` whose subpatch is loading, or null.
fn loading_pfft() -> *mut PfftPub {
    // GAP: pfft~ introspection
    // SAFETY: `gensym` returns a valid symbol that lives as long as Max.
    unsafe { (*max_sys::gensym(c"__pfft~__".as_ptr())).s_thing.cast() }
}

impl MSPObjWrapped for FftinfoTilde {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::signal().assist("(signal) Dummy"));
        let outs = [
            "(int) FFT Frame Size",
            "(int) Spectral Frame Size",
            "(int) FFT Hop Size",
            "(int) Full Spectrum Flag (0/1)",
        ]
        .map(|text| builder.outlet(Outlet::int().assist(text)));
        // GAP: z_misc flags
        // SAFETY: the object is under construction and nothing else touches its header now.
        unsafe { (*builder.msp_obj()).z_misc |= Z_PUT_FIRST };
        let this = Self {
            obj: builder.obj(),
            pfft: AtomicPtr::new(loading_pfft()),
            fftsize: Int64::new(0),
            n: Int64::new(0),
            ffthop: Int64::new(0),
            fullspect: Int64::new(0),
            outs,
        };
        this.read_pfft();
        Ok(this)
    }

    fn dsp_setup(&self, _sample_rate: f64) {
        if self.pfft.load(Ordering::Relaxed).is_null() {
            self.pfft.store(loading_pfft(), Ordering::Relaxed);
        }
        if self.read_pfft() {
            self.output();
        } else {
            self.warn();
        }
    }

    fn perform(&self, _ins: &[&[f64]], _outs: &mut [&mut [f64]], _nframes: usize) {}
}

impl FftinfoTilde {
    // Copies the owning pfft~'s settings; false outside a pfft~.
    fn read_pfft(&self) -> bool {
        let p = self.pfft.load(Ordering::Relaxed);
        if p.is_null() {
            return false;
        }
        // GAP: pfft~ introspection
        // SAFETY: `p` is the pfft~ that bound itself to `__pfft~__` while loading the subpatch
        // this object is in; it outlives the objects in that subpatch.
        let (size, hop, full) = unsafe { ((*p).x_fftsize, (*p).x_ffthop, (*p).x_fullspect) };
        self.fftsize.set(size as t_atom_long);
        self.ffthop.set(hop as t_atom_long);
        self.fullspect.set((full != 0) as t_atom_long);
        let n = if full != 0 { size } else { size / 2 };
        self.n.set(n as t_atom_long);
        true
    }

    fn output(&self) {
        let values = [&self.fftsize, &self.n, &self.ffthop, &self.fullspect];
        for (out, value) in self.outs.iter().zip(values).rev() {
            let _ = out.send(value.get());
        }
    }

    fn warn(&self) {
        if WARN.swap(false, Ordering::Relaxed) {
            self.obj.warn("fftinfo~ only functions inside a pfft~");
        }
    }
}

#[median::methods]
impl FftinfoTilde {
    #[bang]
    pub fn bang(&self) {
        if self.pfft.load(Ordering::Relaxed).is_null() {
            self.warn();
        } else {
            self.output();
        }
    }
}

median::ext_main! { FftinfoTilde }

#[cfg(test)]
mod tests {
    use super::{PfftPub, Z_PUT_FIRST};
    use median::max_sys;
    use median_mock::Atom::Long;

    #[test]
    fn reports_the_owning_pfft() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        // SAFETY: all zeros is a valid `t_pfftpub` (null pointers, zero sizes).
        let mut pfft: PfftPub = unsafe { std::mem::zeroed() };
        pfft.x_fftsize = 1024;
        pfft.x_ffthop = 256;
        let pfft = &raw mut pfft;
        // SAFETY: the mock's symbols live as long as the process; the binding is undone below,
        // before `pfft` goes out of scope.
        let bound = unsafe { max_sys::gensym(c"__pfft~__".as_ptr()) };
        unsafe { (*bound).s_thing = pfft.cast() };
        let x = k.new_object("median.fftinfo~", &[]);
        unsafe { (*bound).s_thing = std::ptr::null_mut() };
        assert!(!x.is_null());
        // SAFETY: an MSP object starts with its `t_pxobject`.
        let misc = unsafe { (*x.cast::<max_sys::t_pxobject>()).z_misc };
        assert_ne!(misc & Z_PUT_FIRST, 0);

        k.send_bang(x).unwrap();
        let out: Vec<_> = k.take_outlet_log().into_iter().map(|e| e.atoms).collect();
        assert_eq!(out, [[Long(0)], [Long(256)], [Long(512)], [Long(1024)]]);

        // SAFETY: `pfft` is live; the object reads it only during the calls below.
        unsafe { (*pfft).x_fullspect = 1 };
        k.dsp64(x, 44100., 64).unwrap();
        let out: Vec<_> = k.take_outlet_log().into_iter().map(|e| e.atoms).collect();
        assert_eq!(out, [[Long(1)], [Long(256)], [Long(1024)], [Long(1024)]]);
        assert!(k.take_console_log().is_empty());
        assert!(k.free_object(x));
    }

    #[test]
    fn warns_once_outside_a_pfft() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.fftinfo~", &[]);
        assert!(!x.is_null());
        let y = k.new_object("median.fftinfo~", &[]);
        k.send_bang(x).unwrap();
        k.send_bang(y).unwrap();
        let adds = k.dsp64(x, 44100., 64).unwrap();
        assert_eq!(adds.len(), 1);
        assert!(k.take_outlet_log().is_empty());
        let log = k.take_console_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].text, "fftinfo~ only functions inside a pfft~");
        assert_eq!(
            k.assist(x, true, 1).as_deref(),
            Some("(int) Spectral Frame Size")
        );
        assert!(k.free_object(x));
        assert!(k.free_object(y));
    }
}
