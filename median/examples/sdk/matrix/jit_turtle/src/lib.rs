//! A port of the SDK's `matrix/jit.turtle`: LOGO-style turtle graphics, reading ASCII command
//! codes and sending `linesegment`, `pensize` and `clear` messages for `jit.lcd`.
//!
//! The C is a Max object with a Jitter obex and no Jitter object, so this is a plain
//! `#[derive(MaxObj)]`. Deviations: the right outlet is an ordinary outlet, not the obex dumpout
//! (median cannot set one), so `get<attribute>` replies and the Jitter wrapper's standard messages
//! (`max_jit_classex_standard_wrap`) are missing; the attributes are not deferred
//! (`JIT_ATTR_GET_DEFER_LOW`, `JIT_ATTR_SET_USURP_LOW`). `origin`, a two-long array attribute,
//! is registered with accessors written here.

use std::{
    ffi::{c_long, c_void},
    sync::Mutex,
};

use median::{
    attr::{AttrBuilder, AttrType},
    max_sys::{self, e_max_errorcodes::*, t_max_err},
    prelude::*,
    sym,
};

// The C's truncated 2 pi, kept so that coordinates round as they do there.
#[allow(clippy::approx_constant)]
const PI2: f64 = 6.2831853;
const MAXSTACK: usize = 512;
// Facing north, towards the top of the drawing.
const NORTH: f64 = -PI2 / 4.;

#[derive(MaxObj)]
#[max(name = "jit.median.turtle")]
pub struct Turtle {
    #[attr(label = "Angle")]
    angle: Int64,
    #[attr(label = "Scale")]
    scale: Int64,
    #[attr(label = "Clear Mode")]
    clearmode: Int64,
    state: Mutex<State>,
    out: Outlet<Any>,
    _dumpout: Outlet<Any>,
}

struct State {
    origin: [t_atom_long; 2],
    // How many `origin` values the last set gave, which the getter reports.
    origincount: usize,
    curstack: usize,
    // Set by `reset`: the next move starts from `origin`.
    stacknew: bool,
    thisangle: [f64; MAXSTACK],
    pensize: [t_atom_long; MAXSTACK],
    stack_x: [t_atom_long; MAXSTACK],
    stack_y: [t_atom_long; MAXSTACK],
}

impl State {
    fn new(origin: [t_atom_long; 2]) -> Self {
        Self {
            origin,
            origincount: 2,
            curstack: 0,
            stacknew: true,
            thisangle: [NORTH; MAXSTACK],
            pensize: [1; MAXSTACK],
            stack_x: [origin[0]; MAXSTACK],
            stack_y: [origin[1]; MAXSTACK],
        }
    }

    /// Moves forward `scale` steps, returning the start and end points.
    fn forward(&mut self, scale: t_atom_long) -> [t_atom_long; 4] {
        let c = self.curstack;
        if self.stacknew {
            self.stack_x[c] = self.origin[0];
            self.stack_y[c] = self.origin[1];
            self.stacknew = false;
        }
        let (x, y) = (self.stack_x[c], self.stack_y[c]);
        let dest_x = (x as f64 + scale as f64 * self.thisangle[c].cos() + 0.5) as t_atom_long;
        let dest_y = (y as f64 + scale as f64 * self.thisangle[c].sin() + 0.5) as t_atom_long;
        self.stack_x[c] = dest_x;
        self.stack_y[c] = dest_y;
        [x, y, dest_x, dest_y]
    }
}

impl MaxObjWrapped for Turtle {
    fn class_setup(class: &mut Class<MaxObjWrapper<Self>>) {
        // GAP: CLASS_ATTR_SYM_VARSIZE
        let mut b = AttrBuilder::new_accessors("origin", AttrType::Int64, origin_get, origin_set);
        b.label(c"Origin");
        let added = b.build().map(|attr| class.add_attribute(attr));
        if !matches!(added, Ok(Ok(()))) {
            median::error!("jit.turtle: could not add the origin attribute");
        }
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("(matrix) in"));
        Ok(Self {
            angle: Int64::new(30),
            scale: Int64::new(10),
            clearmode: Int64::new(1),
            // 80 120 is the middle of the bottom of a default 160x120 `jit.lcd`.
            state: Mutex::new(State::new([80, 120])),
            out: builder.outlet(Outlet::any().assist("(matrix) out")),
            _dumpout: builder.outlet(Outlet::any().assist("dumpout")),
        })
    }
}

#[median::methods]
impl Turtle {
    #[int]
    pub fn int(&self, n: t_atom_long) {
        let mut s = self.state.lock().unwrap();
        let c = s.curstack;
        let angle = self.angle.get() as f64 / 360. * PI2;
        match u8::try_from(n).unwrap_or(0) {
            b'#' => {
                s.pensize[c] += 1;
                let size = s.pensize[c];
                drop(s);
                self.pensize(size);
            }
            b'!' => {
                if s.pensize[c] > 1 {
                    s.pensize[c] -= 1;
                }
                let size = s.pensize[c];
                drop(s);
                self.pensize(size);
            }
            b'F' => {
                let line = s.forward(self.scale.get());
                drop(s);
                let _ = self.out.send(&sym!("linesegment"), &line.map(Atom::from));
            }
            b'f' => {
                s.forward(self.scale.get());
            }
            // A branch past the stack is ignored, silently as in the C.
            b'[' if c < MAXSTACK - 1 => {
                s.stack_x[c + 1] = s.stack_x[c];
                s.stack_y[c + 1] = s.stack_y[c];
                s.thisangle[c + 1] = s.thisangle[c];
                s.curstack += 1;
            }
            b']' if c > 0 => s.curstack -= 1,
            b'+' => s.thisangle[c] += angle,
            b'-' => s.thisangle[c] -= angle,
            b'|' => s.thisangle[c] += 0.5 * PI2,
            _ => {}
        }
    }

    #[gimme("reset")]
    pub fn reset(&self, _atoms: &[Atom]) {
        {
            let mut s = self.state.lock().unwrap();
            let origin = s.origin;
            let origincount = s.origincount;
            *s = State::new(origin);
            s.origincount = origincount;
        }
        self.pensize(1);
        if self.clearmode.get() != 0 {
            let _ = self.out.send(&sym!("clear"), &[]);
        }
    }

    fn pensize(&self, size: t_atom_long) {
        let _ = self.out.send(&sym!("pensize"), &[size.into(), size.into()]);
    }
}

/// The `origin` getter: `origincount` long atoms, in an array allocated as Max expects if the one
/// given is too small.
///
/// # Safety
/// Only Max calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn origin_get(
    w: &MaxObjWrapper<Turtle>,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "origin getter", MAX_ERR_GENERIC as _, || {
        if ac.is_null() || av.is_null() {
            return MAX_ERR_NONE as _;
        }
        let (origin, count) = {
            let s = w.wrapped().state.lock().unwrap();
            (s.origin, s.origincount)
        };
        // SAFETY: `ac` and `av` are the getter's out-parameters; `*av` is null or holds `*ac`
        // atoms, and a new array is the caller's to free with `sysmem_freeptr`.
        unsafe {
            if (*av).is_null() || (*ac as usize) < count {
                let size = count.max(1) * std::mem::size_of::<max_sys::t_atom>();
                // GAP: sysmem_newptr
                let p = max_sys::sysmem_newptr(size as _).cast::<max_sys::t_atom>();
                if p.is_null() {
                    *ac = 0;
                    return MAX_ERR_OUT_OF_MEM as _;
                }
                *av = p;
            }
            *ac = count as c_long;
            for (i, v) in origin.into_iter().take(count).enumerate() {
                (*av).add(i).cast::<Atom>().write(Atom::from(v));
            }
        }
        MAX_ERR_NONE as _
    })
}

/// The `origin` setter: up to two values; a shorter list leaves the rest as they were.
///
/// # Safety
/// Only Max calls it, as an attribute setter, with `av` holding `ac` atoms.
unsafe extern "C" fn origin_set(
    w: &MaxObjWrapper<Turtle>,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "origin setter", MAX_ERR_GENERIC as _, || {
        let atoms: &[Atom] = if ac > 0 && !av.is_null() {
            // SAFETY: Max passes `ac` valid atoms; `Atom` is a transparent `t_atom`.
            unsafe { std::slice::from_raw_parts(av.cast(), ac as usize) }
        } else {
            &[]
        };
        let mut s = w.wrapped().state.lock().unwrap();
        let count = atoms.len().min(2);
        for (v, a) in s.origin.iter_mut().zip(atoms) {
            *v = a.get_int();
        }
        s.origincount = count;
        MAX_ERR_NONE as _
    })
}

median::ext_main! { Turtle }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn send(k: &median_mock::Kernel, x: *mut std::ffi::c_void, commands: &str) {
        for c in commands.bytes() {
            k.send_int(x, c as i64).unwrap();
        }
    }

    fn log(k: &median_mock::Kernel) -> Vec<(usize, String, Vec<Atom>)> {
        k.take_outlet_log()
            .into_iter()
            .map(|e| (e.index, e.selector, e.atoms))
            .collect()
    }

    fn longs(v: &[i64]) -> Vec<Atom> {
        v.iter().map(|&n| Atom::Long(n)).collect()
    }

    #[test]
    fn draws_turns_and_branches() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.turtle", &[]);
        assert!(!x.is_null(), "{:?}", k.console_log());
        // Up 10 from 80 120, up 10 more in a branch, then back at 80 110, right 10.
        k.attr_set(x, "angle", &[Atom::Long(90)]).unwrap();
        send(&k, x, "F[F]+F");
        assert_eq!(
            log(&k),
            [
                (0, "linesegment".into(), longs(&[80, 120, 80, 110])),
                (0, "linesegment".into(), longs(&[80, 110, 80, 100])),
                (0, "linesegment".into(), longs(&[80, 110, 90, 110])),
            ]
        );
        send(&k, x, "#!!f|F");
        assert_eq!(
            log(&k),
            [
                (0, "pensize".into(), longs(&[2, 2])),
                (0, "pensize".into(), longs(&[1, 1])),
                (0, "pensize".into(), longs(&[1, 1])),
                (0, "linesegment".into(), longs(&[100, 110, 90, 110])),
            ]
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn reset_returns_to_origin_and_clears() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object(
            "jit.median.turtle",
            &["@origin".into(), 10i64.into(), 20i64.into()],
        );
        assert!(!x.is_null(), "{:?}", k.console_log());
        assert_eq!(k.attr_get(x, "origin").unwrap(), longs(&[10, 20]));
        assert_eq!(k.attr_get(x, "scale").unwrap(), longs(&[10]));
        send(&k, x, "+F");
        k.take_outlet_log();
        k.attr_set(x, "origin", &[Atom::Long(0), Atom::Long(0)])
            .unwrap();
        k.send(x, "reset", &[]).unwrap();
        send(&k, x, "F");
        assert_eq!(
            log(&k),
            [
                (0, "pensize".into(), longs(&[1, 1])),
                (0, "clear".into(), vec![]),
                (0, "linesegment".into(), longs(&[0, 0, 0, -9])),
            ]
        );
        k.attr_set(x, "clearmode", &[Atom::Long(0)]).unwrap();
        k.attr_set(x, "origin", &[Atom::Long(5)]).unwrap();
        assert_eq!(k.attr_get(x, "origin").unwrap(), longs(&[5]));
        k.send(x, "reset", &[]).unwrap();
        assert_eq!(log(&k), [(0, "pensize".into(), longs(&[1, 1]))]);
        assert_eq!(k.assist(x, false, 0).as_deref(), Some("(matrix) in"));
        assert_eq!(k.assist(x, true, 0).as_deref(), Some("(matrix) out"));
        assert_eq!(k.assist(x, true, 1).as_deref(), Some("dumpout"));
        assert!(k.free_object(x));
    }
}
