//! `buffer~` access through the `median-mock` kernel: the shared claim (`BufferLocked`, atomic
//! samples, interleaved), the exclusive edit (`BufferEdit`), lock and edit pairing (also when a
//! panic unwinds), `set`/`dblclick` from `Class::add_buffer_messages`, and the forwarding of
//! binding notifications to the buffer reference.

use median::buffer::{BufferLocked, BufferRef, BufferReference, EditError, TryLockError};
use median::builder::ManagedBufferRef;
use median::prelude::*;
use median_mock::{Atom as MockAtom, Kernel, Level, Role};
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Once;

/// An MSP object that plays channel 1 of its buffer and writes minus the frame index into
/// channel 0 (mode 0), or claims the buffer and panics (mode 1). `bang` edits the buffer.
#[derive(MaxObject)]
#[max(name = "buftest~", kind = msp)]
pub struct BufTest {
    obj: ObjRef,
    buf: ManagedBufferRef,
    mode: Int64,
}

impl MSPObjWrapped for BufTest {
    fn new(b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        b.outlet(Outlet::signal());
        let name = b.creation_args().first().map(|a| a.get_symbol());
        Ok(Self {
            obj: b.obj(),
            buf: b.with_buffer(name),
            mode: Int64::new(0),
        })
    }

    fn class_setup(c: &mut Class<MSPObjWrapper<Self>>) {
        c.add_buffer_messages(|s: &Self| &*s.buf).unwrap();
    }

    fn perform(&self, _ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let Ok(mut l) = self.buf.try_lock() else {
            outs[0].fill(-1.);
            return;
        };
        if self.mode.get() == 1 {
            panic!("panic while holding the claim");
        }
        for (i, (o, s)) in outs[0].iter_mut().zip(l.channel_iter(1)).enumerate() {
            *o = s.get() as f64;
            l.sample(i, 0).set(-(i as f32));
        }
        l.set_dirty();
    }
}

#[median::methods]
impl BufTest {
    #[int]
    pub fn int(&self, v: t_atom_long) {
        self.mode.set(v);
    }

    /// Edit: every sample becomes `frame + channel / 10`.
    #[bang]
    pub fn bang(&self) -> Result<(), String> {
        let mut e = self.buf.edit().map_err(|e| format!("{e:?}"))?;
        for i in 0..e.frames() {
            for (ch, s) in e.frame_mut(i).iter_mut().enumerate() {
                *s = i as f32 + ch as f32 / 10.;
            }
        }
        median::object_post!(&self.obj, "edited {} frames", e.frames());
        Ok(())
    }
}

median::ext_main! { BufTest }

fn setup() -> Kernel {
    static ONCE: Once = Once::new();
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
    k
}

/// A `buftest~` (for its object pointer) and a `buffer~` "b" of 3 channels and 4 frames whose
/// sample `ch` of frame `i` is `i * 10 + ch`.
fn object_and_buffer(k: &Kernel) -> *mut c_void {
    let obj = k.new_object("buftest~", &[MockAtom::Sym("b".into())]);
    assert!(!obj.is_null());
    k.create_buffer("b", 3, 4, 48000.);
    let data: Vec<f32> = (0..4)
        .flat_map(|i| (0..3).map(move |ch| (i * 10 + ch) as f32))
        .collect();
    k.set_buffer_samples("b", &data);
    obj
}

/// A buffer reference made by hand for `obj`.
fn reference(obj: *mut c_void, name: &str) -> BufferRef {
    let name = SymbolRef::try_from(name).unwrap();
    // SAFETY: `obj` is a live object that outlives the reference in every test
    unsafe { BufferRef::new(obj as _, Some(name)) }
}

fn errors(k: &Kernel) -> Vec<String> {
    k.take_console_log()
        .into_iter()
        .filter(|e| e.level == Level::Error)
        .map(|e| e.text)
        .collect()
}

#[test]
fn the_shared_view_is_interleaved_and_atomic() {
    let k = setup();
    let obj = object_and_buffer(&k);
    let r = reference(obj, "b");
    assert_eq!(
        (r.channels(), r.frames(), r.sample_rate()),
        (Some(3), Some(4), Some(48000.))
    );
    {
        let mut l = r.try_lock().unwrap();
        assert_eq!(k.buffer_state("b"), Some((1, false)));
        assert_eq!((l.frames(), l.channels(), l.sample_rate()), (4, 3, 48000.));
        assert_eq!(l.as_slice().len(), 12);
        let f: Vec<f32> = l.frame(2).iter().map(|s| s.get()).collect();
        assert_eq!(f, [20., 21., 22.]);
        assert_eq!(l.sample(3, 1).get(), 31.);
        //writes land at frame * channels + channel
        l.sample(1, 2).set(-1.);
        l.frame(0)[1].set(-2.);
        assert_eq!(k.buffer_samples("b").unwrap()[5], -1.);
        assert_eq!(k.buffer_samples("b").unwrap()[1], -2.);
        //two claims at once are fine: they are shared
        let l2 = r.try_lock().unwrap();
        assert_eq!(l2.sample(1, 2).get(), -1.);
        assert_eq!(k.buffer_state("b"), Some((2, false)));
        drop(l2);
        // SAFETY: only the pointer is compared
        assert_eq!(
            unsafe { l.samples_ptr() },
            l.as_slice().as_ptr() as *mut f32
        );
        l.set_dirty();
    }
    assert_eq!(k.buffer_state("b"), Some((0, true)));
    drop(r);
    assert!(k.free_object(obj));
    assert!(errors(&k).is_empty());
}

#[test]
fn channel_iter_is_strided() {
    let k = setup();
    let obj = object_and_buffer(&k);
    let r = reference(obj, "b");
    let l = r.try_lock().unwrap();
    let ch = |c: usize| l.channel_iter(c).map(|s| s.get()).collect::<Vec<_>>();
    assert_eq!(ch(0), [0., 10., 20., 30.]);
    assert_eq!(ch(1), [1., 11., 21., 31.]);
    assert_eq!(ch(2), [2., 12., 22., 32.]);
    assert_eq!(l.channel_iter(1).len(), 4);
    let back: Vec<f32> = l.channel_iter(2).rev().map(|s| s.get()).collect();
    assert_eq!(back, [32., 22., 12., 2.]);
    drop(l);
    assert_eq!(k.buffer_state("b"), Some((0, false)));
    drop(r);
    assert!(k.free_object(obj));
}

fn panics_holding_a_claim(k: &Kernel, r: &BufferRef, f: impl Fn(&BufferLocked)) {
    let res = catch_unwind(AssertUnwindSafe(|| {
        let l = r.try_lock().unwrap();
        assert_eq!(k.buffer_state("b"), Some((1, false)));
        f(&l);
    }));
    assert!(res.is_err());
    assert_eq!(k.buffer_state("b"), Some((0, false)));
}

#[test]
fn the_claim_is_released_on_drop_and_on_panic() {
    let k = setup();
    let obj = object_and_buffer(&k);
    let r = reference(obj, "b");
    //out of range: panics, and the unwinding releases the claim
    panics_holding_a_claim(&k, &r, |l| drop(l.channel_iter(3)));
    panics_holding_a_claim(&k, &r, |l| {
        let _ = l.frame(4);
    });
    panics_holding_a_claim(&k, &r, |l| {
        let _ = l.sample(0, 3);
    });
    panics_holding_a_claim(&k, &r, |_| panic!("user panic"));
    //a perform routine that panics while holding the claim: the guard posts the panic, the claim
    //is released (the mock would report a perform that returns holding one)
    k.send_int(obj, 1).unwrap();
    let adds = k.dsp64(obj, 48000., 4).unwrap();
    let mut outs = vec![vec![9.; 4]];
    adds[0].run(&mut [], &mut outs);
    assert_eq!(outs, [[0.; 4]]);
    let e = errors(&k);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].contains("panic while holding the claim"), "{e:?}");
    assert_eq!(k.buffer_state("b"), Some((0, false)));
    drop(r);
    assert!(k.free_object(obj));
}

#[test]
fn perform_reads_and_writes_through_the_claim() {
    let k = setup();
    let obj = object_and_buffer(&k);
    let adds = k.dsp64(obj, 48000., 4).unwrap();
    let mut outs = vec![vec![9.; 4]];
    adds[0].run(&mut [], &mut outs);
    assert_eq!(outs, [[1., 11., 21., 31.]]);
    assert_eq!(
        k.buffer_samples("b").unwrap(),
        [0., 1., 2., -1., 11., 12., -2., 21., 22., -3., 31., 32.]
    );
    assert_eq!(k.buffer_state("b"), Some((0, true)));
    //no buffer: perform's `try_lock` fails
    k.destroy_buffer("b");
    adds[0].run(&mut [], &mut outs);
    assert_eq!(outs, [[-1.; 4]]);
    assert!(errors(&k).is_empty());
    assert!(k.free_object(obj));
}

#[test]
fn an_edit_is_exclusive_and_ends_on_drop_and_on_panic() {
    let k = setup();
    let obj = object_and_buffer(&k);
    let r = reference(obj, "b");
    {
        let mut e = r.edit().unwrap();
        assert_eq!(k.buffer_editing("b"), Some(true));
        assert_eq!((e.frames(), e.channels(), e.sample_rate()), (4, 3, 48000.));
        assert_eq!(e.frame(1), [10., 11., 12.]);
        assert_eq!(
            e.channel_iter(2).copied().collect::<Vec<_>>(),
            [2., 12., 22., 32.]
        );
        //no shared claim while the edit is in progress
        assert_eq!(r.try_lock().err(), Some(TryLockError::BufferDoesNotExist));
        e.frame_mut(0).copy_from_slice(&[7., 8., 9.]);
        for s in e.channel_iter_mut(1) {
            *s = -*s;
        }
        e.as_mut_slice()[11] = 100.;
    }
    assert_eq!(k.buffer_editing("b"), Some(false));
    assert_eq!(k.buffer_edits("b"), Some((1, 1)));
    assert_eq!(k.buffer_state("b"), Some((0, true)));
    assert_eq!(
        k.buffer_samples("b").unwrap(),
        [7., -8., 9., 10., -11., 12., 20., -21., 22., 30., -31., 100.]
    );
    //a read-only edit does not mark it dirty
    k.clear_buffer_dirty("b");
    drop(r.edit().unwrap());
    assert_eq!(k.buffer_state("b"), Some((0, false)));
    //a panic ends the edit
    let res = catch_unwind(AssertUnwindSafe(|| {
        let e = r.edit().unwrap();
        let _ = e.frame(4);
    }));
    assert!(res.is_err());
    assert_eq!(k.buffer_edits("b"), Some((3, 3)));
    assert_eq!(k.buffer_editing("b"), Some(false));
    //from a message on the main thread
    k.take_console_log();
    k.send_bang(obj).unwrap();
    let log = k.take_console_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].text, "edited 4 frames");
    let d = k.buffer_samples("b").unwrap();
    assert_eq!((d[0], d[3], d[5]), (0., 1., 1.2));
    assert!((d[4] - 1.1).abs() < 1e-6);
    assert_eq!(k.buffer_edits("b"), Some((4, 4)));
    drop(r);
    assert!(k.free_object(obj));
    assert!(errors(&k).is_empty());
}

#[test]
fn an_edit_is_refused_on_the_audio_and_scheduler_threads_and_without_a_buffer() {
    let k = setup();
    let obj = object_and_buffer(&k);
    let r = reference(obj, "b");
    for role in [Role::Audio, Role::Timer] {
        let e = k.with_role(role, || r.edit().err());
        assert_eq!(e, Some(EditError::WrongThread));
    }
    //a thread of your own is fine
    assert_eq!(
        k.with_role(Role::Other, || r.edit().map(|e| e.frames())),
        Ok(4)
    );
    assert_eq!(k.buffer_edits("b"), Some((1, 1)));
    let none = reference(obj, "nonesuch");
    assert_eq!(none.edit().err(), Some(EditError::BufferDoesNotExist));
    assert_eq!(
        none.try_lock().err(),
        Some(TryLockError::BufferDoesNotExist)
    );
    assert!(!none.view());
    drop((r, none));
    assert!(k.free_object(obj));
    assert!(errors(&k).is_empty());
}

#[test]
fn set_and_dblclick_messages() {
    let k = setup();
    let obj = k.new_object("buftest~", &[]);
    k.create_buffer("x", 1, 2, 44100.);
    //no buffer named yet: double-click opens nothing
    assert!(k.dblclick(obj));
    assert!(k.buffer_views().is_empty());
    k.send(obj, "set", &[MockAtom::Sym("x".into())]).unwrap();
    assert!(k.dblclick(obj));
    assert_eq!(k.buffer_views(), ["x"]);
    //`set` and `name` on a reference
    let r = reference(obj, "x");
    assert_eq!(r.name(), median::sym!("x"));
    assert!(r.exists());
    r.set(median::sym!("y"));
    assert_eq!(r.name(), median::sym!("y"));
    assert!(!r.exists());
    drop(r);
    assert!(k.free_object(obj));
    assert!(errors(&k).is_empty());
}

#[test]
fn binding_notifications_reach_the_buffer_reference() {
    let k = setup();
    let obj = k.new_object("buftest~", &[MockAtom::Sym("b".into())]);
    let ours = k.create_buffer("b", 3, 4, 48000.);
    let b = k.create_buffer("other", 1, 1, 44100.);
    let (notify, _) = k.method(obj, "notify").unwrap();
    // SAFETY: `notify` is the `A_CANT` method `(x, s, msg, sender, data)`
    let notify = unsafe {
        std::mem::transmute::<
            usize,
            extern "C" fn(
                *mut c_void,
                *mut max_sys::t_symbol,
                *mut max_sys::t_symbol,
                *mut c_void,
                *mut c_void,
            ),
        >(notify)
    };
    let before = k.buffer_ref_notifications().len();
    //for another buffer: not forwarded
    notify(
        obj,
        median::sym!("other").as_ptr(),
        median::sym!("globalsymbol_binding").as_ptr(),
        b as _,
        b as _,
    );
    assert_eq!(k.buffer_ref_notifications().len(), before);
    //for ours: forwarded
    let n = k.buffer_ref_notifications().len();
    notify(
        obj,
        median::sym!("b").as_ptr(),
        median::sym!("globalsymbol_binding").as_ptr(),
        ours as _,
        ours as _,
    );
    let notes = k.buffer_ref_notifications();
    assert_eq!(notes.len(), n + 1);
    assert_eq!(
        (notes[n].1.as_str(), notes[n].2.as_str()),
        ("b", "globalsymbol_binding")
    );
    //not a binding message: not forwarded (the user's handler still gets it)
    notify(
        obj,
        median::sym!("b").as_ptr(),
        median::sym!("buffer_modified").as_ptr(),
        ours as _,
        ours as _,
    );
    assert_eq!(k.buffer_ref_notifications().len(), n + 1);
    assert!(k.free_object(obj));
}

#[test]
fn a_second_edit_of_a_buffer_is_busy_through_any_reference() {
    let k = setup();
    let obj = object_and_buffer(&k);
    let r = reference(obj, "b");
    let r2 = std::sync::Arc::new(reference(obj, "b"));
    k.create_buffer("other", 1, 2, 48000.);
    let other = reference(obj, "other");
    {
        let e = r.edit().unwrap();
        //the same reference, another reference to the same `buffer~`, a clone of an `Arc`
        assert_eq!(r.edit().err(), Some(EditError::Busy));
        assert_eq!(r2.edit().err(), Some(EditError::Busy));
        let r3 = r2.clone();
        assert_eq!(r3.edit().err(), Some(EditError::Busy));
        //and from another thread
        let busy = std::thread::scope(|s| s.spawn(|| r2.edit().err()).join().unwrap());
        assert_eq!(busy, Some(EditError::Busy));
        //another buffer is not affected
        assert_eq!(other.edit().map(|e| e.frames()), Ok(2));
        drop(e);
    }
    //`buffer_edit_begin` was never called a second time (the mock reports a nested begin)
    assert_eq!(k.buffer_edits("b"), Some((1, 1)));
    assert!(errors(&k).is_empty());
    //free again once the edit ended, also after a panic
    assert!(r2.edit().is_ok());
    let res = catch_unwind(AssertUnwindSafe(|| {
        let _e = r.edit().unwrap();
        panic!("while editing");
    }));
    assert!(res.is_err());
    assert!(r2.edit().is_ok());
    assert_eq!(k.buffer_edits("b"), Some((4, 4)));
    drop((r, r2, other));
    assert!(k.free_object(obj));
    assert!(errors(&k).is_empty());
}

#[test]
fn an_empty_multichannel_buffer_has_empty_channels_and_keeps_its_valid_flag() {
    let k = setup();
    let obj = object_and_buffer(&k);
    k.create_buffer("empty", 3, 0, 48000.);
    let r = reference(obj, "empty");
    {
        let l = r.try_lock().unwrap();
        assert_eq!((l.frames(), l.channels()), (0, 3));
        for ch in 0..3 {
            assert_eq!(l.channel_iter(ch).count(), 0);
        }
    }
    {
        let mut e = r.edit().unwrap();
        assert_eq!((e.frames(), e.channels()), (0, 3));
        assert_eq!(e.channel_iter(2).count(), 0);
        assert_eq!(e.channel_iter_mut(2).count(), 0);
    }
    //`buffer_edit_end(b, -1)`: restore the old valid flag of a buffer without samples
    assert_eq!(k.buffer_last_edit_valid("empty"), Some(Some(-1)));
    //and `1` for one with samples
    let b = reference(obj, "b");
    drop(b.edit().unwrap());
    assert_eq!(k.buffer_last_edit_valid("b"), Some(Some(1)));
    drop((r, b));
    assert!(k.free_object(obj));
    assert!(errors(&k).is_empty());
}
