//! A port of the SDK's `mc/mc.pack~`: combines its inlets into one multichannel signal.
//! `median.mc.pack~` takes the first channel of each inlet; `median.mc.combine~`, the same object
//! under another name, takes every channel. A float or list sets an inlet's value, used when the
//! inlet has no signal.
//!
//! The SDK object reads, in `dsp64`, which inlets are connected (`count[]`) and how many channels
//! each has (`getnuminputchannels` on the DSP chain), neither of which median passes on. Here an
//! inlet's channel count is the last one `inputchanged` reported for it (one until then, which is
//! what Max gives an unconnected inlet), and a float or list replaces the inlet's signal from then
//! on, as in the other ports without `count[]`. An unconnected inlet reads zeros, the default value,
//! so the port differs from the SDK only where an inlet that has had a value is also connected.
//! `inputchanged` records the count with a fixed `chans` too (the SDK relies on `dsp64` for that).
//!
//! median has no multichannel outlet, so the outlet is made raw and is not in median's iolet table:
//! its assist text comes from the `assist` hook. `perform` is handed at most 64 channels each way
//! (median's `MAX_SIGNAL_IO`), and more than 64 inlets are refused at `dsp64` the same way: with
//! more, median posts an error and the output is silent, where the SDK allows 1024. Output channels
//! no input reaches are zeroed; `mc.combine~` in the SDK leaves them unwritten.
//!
//! The SDK registers `mc.pack~` twice and `mc.combine~` never; the port registers both. Max finds
//! `median.mc.combine~` through `init/median-mc-pack.txt`. Untested: everything Max does with the
//! outlet type, `Z_MC_INLETS`, the channel protocol and `helpname`; the tests call
//! `inputchanged`, `multichanneloutputs` and `helpname` directly.

use std::{
    borrow::Cow,
    ffi::{CStr, c_char, c_int, c_long, c_short, c_void},
    sync::atomic::{AtomicBool, Ordering},
};

use median::{max_sys, prelude::*};

// From `z_dsp.h`, which max-sys binds without its macros.
const Z_MC_INLETS: c_short = 32;
const MC_MAX_CHANS: t_atom_long = 1024;

// The state both classes share; `combine` is fixed by the class.
struct Pack {
    combine: bool,
    values: Box<[Float64]>,
    // Whether `values` replaces the inlet's signal.
    replaced: Box<[AtomicBool]>,
    // Input channels per inlet, as `inputchanged` last reported.
    chans: Box<[Int64]>,
    // The `chans` attribute: output channels, or 0 to adapt.
    fixed: Int64,
    muted: AtomicBool,
}

impl Pack {
    fn new(builder: &mut MSPBuilder<impl MSPObjWrapped>, combine: bool) -> Result<Self, NewError> {
        let size = builder.args().at(0).unwrap_or(2)?.clamp(1, MC_MAX_CHANS) as usize;
        for _ in 0..size {
            builder.inlet(Inlet::signal().assist("(signal, float) Input"));
        }
        // GAP: multichannel signal outlets
        // SAFETY: the object is under construction; the symbol is interned.
        let out = unsafe {
            max_sys::outlet_append(
                builder.max_obj(),
                std::ptr::null_mut(),
                max_sys::gensym(c"multichannelsignal".as_ptr()),
            )
        };
        if out.is_null() {
            return Err(NewError::new("Max did not make the multichannel outlet"));
        }
        // GAP: z_misc flags
        // SAFETY: the object is under construction and nothing else touches its header now.
        unsafe { (*builder.msp_obj()).z_misc |= Z_MC_INLETS };
        Ok(Self {
            combine,
            values: (0..size).map(|_| Float64::new(0.)).collect(),
            replaced: (0..size).map(|_| AtomicBool::new(false)).collect(),
            chans: (0..size).map(|_| Int64::new(1)).collect(),
            fixed: Int64::new(0),
            muted: AtomicBool::new(false),
        })
    }

    // The channels an inlet contributes to `ins`.
    fn inlet_chans(&self, inlet: usize) -> usize {
        self.chans[inlet].get().clamp(1, MC_MAX_CHANS) as usize
    }

    fn output_chans(&self) -> c_long {
        match self.fixed.get() {
            0 if self.combine => (0..self.chans.len())
                .map(|i| self.inlet_chans(i))
                .sum::<usize>()
                .min(MC_MAX_CHANS as usize) as c_long,
            0 => self.chans.len() as c_long,
            n => n as c_long,
        }
    }

    fn inputchanged(&self, index: c_long, chans: c_long) -> bool {
        let Some(slot) = usize::try_from(index).ok().and_then(|i| self.chans.get(i)) else {
            return false;
        };
        let changed = slot.get() != chans as t_atom_long;
        slot.set(chans as t_atom_long);
        changed && self.fixed.get() == 0
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]]) {
        let muted = self.muted.load(Ordering::Relaxed);
        // The first channel of each inlet in `ins`.
        let mut start = 0;
        let mut written = 0;
        for inlet in 0..self.chans.len() {
            let n = self.inlet_chans(inlet);
            let used = if self.combine { n } else { 1 };
            let value = self.values[inlet].get();
            let signal = !self.replaced[inlet].load(Ordering::Relaxed);
            for ch in 0..used {
                let Some(out) = outs.get_mut(written) else {
                    return;
                };
                match ins.get(start + ch) {
                    _ if muted => out.fill(0.),
                    Some(input) if signal => out.copy_from_slice(input),
                    _ => out.fill(value),
                }
                written += 1;
            }
            start += n;
        }
        for out in outs.iter_mut().skip(written) {
            out.fill(0.);
        }
    }

    fn float(&self, inlet: usize, f: f64) {
        if let (Some(v), Some(r)) = (self.values.get(inlet), self.replaced.get(inlet)) {
            v.set(f);
            r.store(true, Ordering::Relaxed);
        }
    }
}

// Gives the shared trampolines the state of either class.
trait HasPack: MSPObjWrapped {
    fn pack(&self) -> &Pack;
}

macro_rules! pack_class {
    ($t:ident, $name:literal, $combine:literal) => {
        #[derive(MSPObj)]
        #[max(name = $name)]
        pub struct $t {
            obj: ObjRef,
            pack: Pack,
        }

        impl HasPack for $t {
            fn pack(&self) -> &Pack {
                &self.pack
            }
        }

        impl MSPObjWrapped for $t {
            fn class_setup(class: &mut Class<MSPObjWrapper<Self>>) {
                // SAFETY: the class is the live class being set up.
                unsafe { add_methods::<Self>(class.inner()) };
            }

            fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
                Ok(Self {
                    obj: builder.obj(),
                    pack: Pack::new(builder, $combine)?,
                })
            }

            fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
                self.pack.perform(ins, outs);
            }

            fn assist(&self, iolet: IoLet) -> Option<Cow<'_, str>> {
                match iolet {
                    IoLet::Outlet(0) => Some("(multi-channel signal) Output".into()),
                    _ => None,
                }
            }
        }

        #[median::methods]
        impl $t {
            #[int]
            pub fn int(&self, n: t_atom_long) {
                self.float(n as f64);
            }

            #[float]
            pub fn float(&self, f: f64) {
                self.pack.float(self.obj.proxy_inlet(), f);
            }

            #[list]
            pub fn list(&self, atoms: &[Atom]) {
                let start = self.obj.proxy_inlet();
                for (i, a) in atoms.iter().enumerate() {
                    self.pack.float(start + i, a.get_float());
                }
            }

            #[sel("mute")]
            pub fn mute(&self, way: t_atom_long) {
                self.pack.muted.store(way != 0, Ordering::Relaxed);
            }

            #[attr(clip(0, MC_MAX_CHANS), label = "Output Channels")]
            pub fn chans(&self) -> t_atom_long {
                self.pack.fixed.get()
            }

            pub fn set_chans(&self, n: t_atom_long) {
                self.pack.fixed.set(n);
            }
        }
    };
}

pack_class!(McPackTilde, "median.mc.pack~", false);
pack_class!(McCombineTilde, "median.mc.combine~", true);

/// # Safety
/// `class` must be the live class of `T`, being set up.
unsafe fn add_methods<T: HasPack>(class: *mut max_sys::t_class) {
    // GAP: MC channel protocol (multichanneloutputs, inputchanged)
    // GAP: A_CANT methods with a custom selector or arguments (helpname)
    // SAFETY: each function has the signature Max calls the `A_CANT` method of that name with;
    // the caller's contract.
    unsafe {
        add_cant(
            class,
            c"multichanneloutputs",
            std::mem::transmute::<
                unsafe extern "C" fn(*mut MSPObjWrapper<T>, c_long) -> c_long,
                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            >(multichanneloutputs::<T>),
        );
        add_cant(
            class,
            c"inputchanged",
            std::mem::transmute::<
                unsafe extern "C" fn(*mut MSPObjWrapper<T>, c_long, c_long) -> c_long,
                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            >(inputchanged::<T>),
        );
        add_cant(
            class,
            c"helpname",
            std::mem::transmute::<
                unsafe extern "C" fn(*mut MSPObjWrapper<T>, *mut c_char),
                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            >(helpname::<T>),
        );
    }
}

/// # Safety
/// `class` must be a live class; `f` must have the signature Max calls `sel` with.
unsafe fn add_cant(
    class: *mut max_sys::t_class,
    sel: &CStr,
    f: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
) {
    // SAFETY: the caller's contract
    unsafe {
        max_sys::class_addmethod(
            class,
            Some(f),
            sel.as_ptr(),
            max_sys::e_max_atomtypes::A_CANT as c_int,
            0,
        );
    }
}

/// # Safety
/// `x` must be a live object of `T`'s class.
unsafe extern "C" fn multichanneloutputs<T: HasPack>(
    x: *mut MSPObjWrapper<T>,
    _index: c_long,
) -> c_long {
    // SAFETY: the caller's contract
    let w = unsafe { &*x };
    median::ffi::guard_or(w, "multichanneloutputs", 1, || {
        w.wrapped().pack().output_chans()
    })
}

/// # Safety
/// `x` must be a live object of `T`'s class.
unsafe extern "C" fn inputchanged<T: HasPack>(
    x: *mut MSPObjWrapper<T>,
    index: c_long,
    chans: c_long,
) -> c_long {
    // SAFETY: the caller's contract
    let w = unsafe { &*x };
    median::ffi::guard_or(w, "inputchanged", 0, || {
        w.wrapped().pack().inputchanged(index, chans) as c_long
    })
}

/// Writes the help patcher's name, the class name (the SDK's names are `mc.pack~` and
/// `mc.combine~`).
///
/// # Safety
/// `x` must be a live object of `T`'s class and `s` a buffer Max passes for a file name.
unsafe extern "C" fn helpname<T: HasPack>(x: *mut MSPObjWrapper<T>, s: *mut c_char) {
    // SAFETY: the caller's contract
    let w = unsafe { &*x };
    median::ffi::guard(w, "helpname", || {
        let name = if w.wrapped().pack().combine {
            c"median.mc.combine~"
        } else {
            c"median.mc.pack~"
        };
        let bytes = name.to_bytes_with_nul();
        // SAFETY: the caller's contract; a file name buffer holds far more than these names.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr().cast(), s, bytes.len()) };
    })
}

median::ext_main! { McPackTilde, McCombineTilde }

#[cfg(test)]
mod tests {
    use super::Z_MC_INLETS;
    use median::max_sys;
    use median_mock::Atom::{Float, Long};
    use std::ffi::{CStr, c_char, c_long, c_void};

    type Outputs = unsafe extern "C" fn(*mut c_void, c_long) -> c_long;
    type Changed = unsafe extern "C" fn(*mut c_void, c_long, c_long) -> c_long;
    type Helpname = unsafe extern "C" fn(*mut c_void, *mut c_char);

    fn methods(k: &median_mock::Kernel, x: *mut c_void) -> (Outputs, Changed) {
        let (outputs, _) = k.method(x, "multichanneloutputs").unwrap();
        let (changed, _) = k.method(x, "inputchanged").unwrap();
        // SAFETY: the class registered these two `A_CANT` methods with these signatures.
        unsafe {
            (
                std::mem::transmute::<usize, Outputs>(outputs),
                std::mem::transmute::<usize, Changed>(changed),
            )
        }
    }

    fn helpname(k: &median_mock::Kernel, x: *mut c_void) -> String {
        let (f, _) = k.method(x, "helpname").unwrap();
        let mut buf = [0 as c_char; 512];
        // SAFETY: `helpname` is `A_CANT` `(x, char *)`; `buf` is large enough.
        unsafe {
            let f = std::mem::transmute::<usize, Helpname>(f);
            f(x, buf.as_mut_ptr());
            CStr::from_ptr(buf.as_ptr()).to_str().unwrap().to_owned()
        }
    }

    fn setup() -> median_mock::Kernel {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        k
    }

    #[test]
    fn registers_both_classes() {
        let k = setup();
        for (class, help) in [
            ("median.mc.pack~", "median.mc.pack~"),
            ("median.mc.combine~", "median.mc.combine~"),
        ] {
            let x = k.new_object(class, &[Long(3)]);
            assert!(!x.is_null());
            assert_eq!(k.signal_inlets(x), Some(3));
            assert_eq!(k.outlet_types(x), [Some("multichannelsignal".to_string())]);
            // SAFETY: an MSP object starts with its `t_pxobject`.
            let misc = unsafe { (*x.cast::<max_sys::t_pxobject>()).z_misc };
            assert_eq!(misc & (Z_MC_INLETS | 1), Z_MC_INLETS | 1);
            assert_eq!(
                k.assist(x, false, 2).as_deref(),
                Some("(signal, float) Input")
            );
            assert_eq!(
                k.assist(x, true, 0).as_deref(),
                Some("(multi-channel signal) Output")
            );
            assert_eq!(helpname(&k, x), help);
            assert!(k.free_object(x));
        }
        let x = k.new_object("median.mc.pack~", &[]);
        assert_eq!(k.signal_inlets(x), Some(2));
        assert!(k.free_object(x));
    }

    #[test]
    fn channel_counts() {
        let k = setup();
        let pack = k.new_object("median.mc.pack~", &[Long(3)]);
        let combine = k.new_object("median.mc.combine~", &[Long(3)]);
        let (outputs, changed) = methods(&k, pack);
        // SAFETY: each object is live and is called through its own class' methods.
        unsafe {
            assert_eq!(outputs(pack, 0), 3);
            assert_eq!(changed(pack, 1, 4), 1);
            assert_eq!(changed(pack, 1, 4), 0);
            assert_eq!(changed(pack, 7, 4), 0);
            assert_eq!(outputs(pack, 0), 3);

            let (outputs, changed) = methods(&k, combine);
            assert_eq!(outputs(combine, 0), 3);
            assert_eq!(changed(combine, 0, 2), 1);
            assert_eq!(changed(combine, 2, 5), 1);
            assert_eq!(outputs(combine, 0), 8);
            assert_eq!(changed(combine, 2, 2000), 1);
            assert_eq!(outputs(combine, 0), 1024);

            k.attr_set(combine, "chans", &[Long(6)]).unwrap();
            assert_eq!(changed(combine, 2, 4), 0);
            assert_eq!(outputs(combine, 0), 6);
            k.attr_set(combine, "chans", &[Long(5000)]).unwrap();
            assert_eq!(k.attr_get(combine, "chans").unwrap(), [Long(1024)]);
            k.attr_set(combine, "chans", &[Long(0)]).unwrap();
            // The count reported while `chans` was fixed is kept.
            assert_eq!(outputs(combine, 0), 7);
        }
        assert!(k.free_object(pack));
        assert!(k.free_object(combine));
    }

    fn chans(k: &median_mock::Kernel, x: *mut c_void, counts: &[c_long]) {
        let (_, changed) = methods(k, x);
        for (i, &n) in counts.iter().enumerate() {
            // SAFETY: `x` is live and of the class the method belongs to.
            unsafe { changed(x, i as c_long, n) };
        }
    }

    fn block(chans: &[usize]) -> Vec<Vec<f64>> {
        let mut ins = Vec::new();
        for (inlet, &n) in chans.iter().enumerate() {
            for ch in 0..n {
                ins.push(vec![(10 * (inlet + 1) + ch) as f64; 2]);
            }
        }
        ins
    }

    #[test]
    fn pack_takes_the_first_channel_of_each_inlet() {
        let k = setup();
        let x = k.new_object("median.mc.pack~", &[Long(3)]);
        let adds = k.dsp64(x, 44100., 2).unwrap();

        let mut ins = block(&[1, 1, 1]);
        let mut outs = vec![vec![9.; 2]; 3];
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs, [[10.; 2], [20.; 2], [30.; 2]]);

        chans(&k, x, &[2, 3, 1]);
        let mut ins = block(&[2, 3, 1]);
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs, [[10.; 2], [20.; 2], [30.; 2]]);

        k.send_inlet(x, 1, "float", &[Float(0.5)]).unwrap();
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs, [[10.; 2], [0.5; 2], [30.; 2]]);

        // With `chans` larger than the inlets, the rest are silent.
        k.attr_set(x, "chans", &[Long(5)]).unwrap();
        let mut outs = vec![vec![9.; 2]; 5];
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs, [[10.; 2], [0.5; 2], [30.; 2], [0.; 2], [0.; 2]]);

        k.send(x, "mute", &[Long(1)]).unwrap();
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs, vec![vec![0.; 2]; 5]);
        assert!(k.free_object(x));
    }

    #[test]
    fn combine_takes_every_channel() {
        let k = setup();
        let x = k.new_object("median.mc.combine~", &[Long(2)]);
        let adds = k.dsp64(x, 44100., 2).unwrap();

        chans(&k, x, &[3, 2]);
        let mut ins = block(&[3, 2]);
        let mut outs = vec![vec![9.; 2]; 5];
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs, [[10.; 2], [11.; 2], [12.; 2], [20.; 2], [21.; 2]]);

        // A list from inlet 0 sets both values; one past the last inlet is dropped.
        k.send_inlet(x, 0, "list", &[Float(0.25), Long(2), Long(3)])
            .unwrap();
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs, [[0.25; 2], [0.25; 2], [0.25; 2], [2.; 2], [2.; 2]]);

        // Fewer outputs than channels: the rest are dropped; more: zeroed.
        let mut short = vec![vec![9.; 2]; 2];
        adds[0].run(&mut ins, &mut short);
        assert_eq!(short, [[0.25; 2], [0.25; 2]]);
        let mut long = vec![vec![9.; 2]; 6];
        adds[0].run(&mut ins, &mut long);
        assert_eq!(long[5], [0.; 2]);

        k.send_inlet(x, 1, "int", &[Long(7)]).unwrap();
        adds[0].run(&mut ins, &mut outs);
        assert_eq!(outs[4], [7.; 2]);
        assert!(k.free_object(x));
    }
}
