//! A port of the SDK's `matrix/jit.p.bounds`: keeps particles inside bounds. A particle matrix
//! has two rows (current and previous positions) of cells holding an id, a life and coordinates;
//! a coordinate past `bounds_lo`/`bounds_hi` bounces (mode 0, damped by `squish` and
//! `squish_var`), kills the particle (1), wraps (2) or is clipped (3). Particles after the first
//! with id 0 are dropped.
//!
//! The arrays (`jit_attr_offset_array` in the C) are registered with hand-written accessors; an
//! `x` in `bounds_hi`/`bounds_lo` disables that bound. `mode` is an int attribute (char in the
//! C). The MOP's input and output accept float32 and float64 with at least 2 planes and 2
//! dimensions, set on the ios with raw calls; median-mock answers `getinput`/`getoutput` with
//! null, so that is untested. As in the C, `calc` gives the output the input's type, plane count
//! and dimensions with raw `type`/`planecount`/`dim` calls; the mock rejects those, so only a
//! matching output is tested.
//!
//! Deviations: the C's Max wrapper passes `MAX_JIT_MOP_FLAGS_OWN_ADAPT`; median's uses the
//! default MOP adaptation, which gives the output the same shape before `calc` does. The output
//! is cleared in Rust rather than with the matrix's `clear`. An object-local generator stands in
//! for Jitter's global `jit_rand`, and the random squish variation is the intended -1..1 (the C
//! mixes signed and unsigned arithmetic and gets about -2..0). Counts are clamped to the 30 slots
//! (the C overflows them).

use core::ffi::{c_long, c_void};
use median::{
    atom::AtomType,
    attr::{AttrBuilder, AttrTrampGetMethod, AttrTrampSetMethod, AttrType},
    error::MaxError,
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix, MatrixDataType, MatrixInfo, iter::JitEntryType},
        wrapper::{Core, MatrixKind},
    },
    max_sys::{self, t_jit_error_code as err, t_max_err},
    prelude::*,
};
use std::{
    ffi::CStr,
    ops::{Add, Mul, Sub},
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

/// `JIT_MATRIX_MAX_PLANECOUNT` less the id and life planes.
const SLOTS: usize = 30;
const HI: usize = 0;
const LO: usize = 1;
const SQUISH: usize = 0;
const SQUISH_VAR: usize = 1;
const HALF_RAND: i64 = 4_294_967_294 / 2;

#[derive(Clone, Copy)]
struct Bounds {
    values: [f64; SLOTS],
    enabled: [bool; SLOTS],
    count: usize,
}

#[derive(Clone, Copy)]
struct FloatArray {
    values: [f64; SLOTS],
    count: usize,
}

#[derive(JitMatrixObj)]
#[max(name = "jit.median.p.bounds", no_methods)]
pub struct PBounds {
    #[attr(label = "Mode")]
    mode: Int64,
    /// Indexed by `HI` and `LO`.
    bounds: [Mutex<Bounds>; 2],
    /// Indexed by `SQUISH` and `SQUISH_VAR`.
    squish: [Mutex<FloatArray>; 2],
    seed: AtomicU32,
}

type Getter = AttrTrampGetMethod<Core<PBounds, MatrixKind>>;
type Setter = AttrTrampSetMethod<Core<PBounds, MatrixKind>>;

impl MatrixOp for PBounds {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        let bounds = |value| {
            Mutex::new(Bounds {
                values: [value; SLOTS],
                enabled: [false; SLOTS],
                count: 0,
            })
        };
        let array = |value| {
            Mutex::new(FloatArray {
                values: [value; SLOTS],
                count: 0,
            })
        };
        Ok(Self {
            mode: 0i64.into(),
            bounds: [bounds(1.0), bounds(-1.0)],
            squish: [array(1.0), array(0.0)],
            seed: AtomicU32::new(0),
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }

    fn mop_setup(mop: &mut MOP<'_>) -> JitResult<()> {
        let types: [Atom; 2] = [sym!("float32"), sym!("float64")].map(Atom::from);
        let two = [Atom::from(2i64)];
        for getter in [sym!("getinput"), sym!("getoutput")] {
            // SAFETY: `mop` is a live MOP being configured during registration; the atoms
            // outlive the calls, which copy them.
            // GAP: per-io MOP configuration
            unsafe {
                let io = max_sys::jit_object_method(mop.as_ptr(), getter.as_ptr(), 1 as c_long);
                if io.is_null() {
                    continue;
                }
                for (selector, atoms) in [
                    (sym!("types"), &types[..]),
                    (sym!("minplanecount"), &two[..]),
                    (sym!("mindimcount"), &two[..]),
                ] {
                    max_sys::jit_object_method(
                        io,
                        selector.as_ptr(),
                        atoms.len() as c_long,
                        atoms.as_ptr(),
                    );
                }
            }
        }
        Ok(())
    }

    fn class_setup(class: &mut Class<Core<Self, MatrixKind>>) -> MaxResult<()> {
        let arrays: [(&str, &'static CStr, Getter, Setter); 4] = [
            (
                "bounds_hi",
                c"Bounds High",
                bounds_get::<HI>,
                bounds_set::<HI>,
            ),
            (
                "bounds_lo",
                c"Bounds Low",
                bounds_get::<LO>,
                bounds_set::<LO>,
            ),
            (
                "squish",
                c"Squish",
                squish_get::<SQUISH>,
                squish_set::<SQUISH>,
            ),
            (
                "squish_var",
                c"Squish Var",
                squish_get::<SQUISH_VAR>,
                squish_set::<SQUISH_VAR>,
            ),
        ];
        for (name, label, get, set) in arrays {
            // GAP: CLASS_ATTR_SYM_VARSIZE (array attributes)
            let mut b = AttrBuilder::new_accessors(name, AttrType::Float64, get, set);
            b.label(label);
            class.add_attribute(b.build().map_err(|_| MaxError::Generic)?)?;
        }
        Ok(())
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let input = inputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let in_info = input.lock().info()?;
        let out_info = output.lock().info()?;
        // SAFETY: the output is a live matrix, unlocked here and owned by this call.
        unsafe { conform(output, &in_info, &out_info)? };
        match in_info.kind()? {
            MatrixDataType::Float32 => self.process::<f32>(input, output),
            MatrixDataType::Float64 => self.process::<f64>(input, output),
            MatrixDataType::Char => clear::<u8>(output).and(Err(err::JIT_ERR_MISMATCH_TYPE)),
            MatrixDataType::Int32 => clear::<i32>(output).and(Err(err::JIT_ERR_MISMATCH_TYPE)),
        }
    }
}

/// Gives `output` the type, plane count and dimensions of `input` where they differ.
///
/// # Safety
/// `output` must not be locked or borrowed.
unsafe fn conform(output: &mut Matrix<'_>, input: &MatrixInfo, out: &MatrixInfo) -> JitResult<()> {
    let send = |selector: SymbolRef, atoms: &[Atom]| {
        // SAFETY: a live matrix (the caller's contract); the atoms outlive the call.
        // GAP: matrix resizing (setinfo, type, planecount, dim)
        unsafe {
            max_sys::jit_object_method(
                output.as_ptr(),
                selector.as_ptr(),
                atoms.len() as c_long,
                atoms.as_ptr(),
            );
        }
    };
    let (kind, planes) = (input.kind()?, input.planes()?);
    if kind != out.kind()? {
        send(sym!("type"), &[kind.symbol().into()]);
    }
    if planes != out.planes()? {
        send(sym!("planecount"), &[(planes as i64).into()]);
    }
    if input.dimensions()? != out.dimensions()? {
        let dims: Vec<Atom> = input
            .dimensions()?
            .iter()
            .map(|&d| (d as usize as i64).into())
            .collect();
        send(sym!("dim"), &dims);
    }
    Ok(())
}

fn clear<T: JitEntryType + Default>(output: &mut Matrix<'_>) -> JitResult<()> {
    let mut lock = output.lock();
    let mut data = lock.data().map_err(|_| err::JIT_ERR_INVALID_OUTPUT)?;
    // SAFETY: the output belongs to this matrix_calc call and nothing else borrows it.
    for mut chunk in unsafe { data.chunks_mut::<T>()? } {
        for cell in chunk.entry_iter_mut() {
            cell.fill(T::default());
        }
    }
    Ok(())
}

trait Real:
    JitEntryType + Default + PartialOrd + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self>
{
    fn of(v: f64) -> Self;
}
impl Real for f32 {
    fn of(v: f64) -> Self {
        v as f32
    }
}
impl Real for f64 {
    fn of(v: f64) -> Self {
        v
    }
}

// The C's CLAMP, MAX and MIN macros, which differ from `clamp`/`max`/`min` on NaN.
fn clamp<T: Real>(x: T, lo: T, hi: T) -> T {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}
fn max<T: Real>(a: T, b: T) -> T {
    if a > b { a } else { b }
}
fn min<T: Real>(a: T, b: T) -> T {
    if a < b { a } else { b }
}

fn lcg(state: u32) -> u32 {
    state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// The C's `jit_p_bounds_random_num`: steps the generator, returns -1..1.
fn random(state: &mut u32) -> f32 {
    *state = lcg(*state);
    (*state as i64 - HALF_RAND) as f32 / HALF_RAND as f32
}

struct Params<T> {
    hi: [T; SLOTS],
    lo: [T; SLOTS],
    enable_hi: [bool; SLOTS],
    enable_lo: [bool; SLOTS],
    squish: [T; SLOTS],
    squish_var: [T; SLOTS],
    mode: isize,
}

impl PBounds {
    fn params<T: Real>(&self) -> Params<T> {
        let (hi, lo) = (
            *self.bounds[HI].lock().unwrap(),
            *self.bounds[LO].lock().unwrap(),
        );
        let squish = self.squish[SQUISH].lock().unwrap().values;
        let squish_var = self.squish[SQUISH_VAR].lock().unwrap().values;
        Params {
            hi: hi.values.map(T::of),
            lo: lo.values.map(T::of),
            enable_hi: hi.enabled,
            enable_lo: lo.enabled,
            squish: squish.map(T::of),
            squish_var: squish_var.map(T::of),
            mode: self.mode.get(),
        }
    }

    /// Steps the stand-in for `jit_rand`.
    fn next_seed(&self) -> u32 {
        let previous = self
            .seed
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |s| Some(lcg(s)))
            .unwrap_or_default();
        lcg(previous)
    }

    fn process<T: Real>(&self, input: &mut Matrix<'_>, output: &mut Matrix<'_>) -> JitResult<()> {
        let p = self.params::<T>();
        let (in_info, cells) = {
            let mut lock = input.lock();
            let data = lock.data().map_err(|_| err::JIT_ERR_INVALID_INPUT)?;
            let mut cells = Vec::new();
            // SAFETY: inputs are stable and unwritten while matrix_calc runs; the borrow ends
            // here, before any output borrow, so an in-place output cannot alias it.
            for chunk in unsafe { data.chunks::<T>()? } {
                for cell in chunk.entry_iter() {
                    cells.extend_from_slice(cell);
                }
            }
            (*data.info(), cells)
        };
        let mut lock = output.lock();
        let mut data = lock.data().map_err(|_| err::JIT_ERR_INVALID_OUTPUT)?;
        let out_info = *data.info();
        let dims = in_info.dimensions()?;
        let planes = in_info.planes()?;
        let shaped = out_info.kind()? == T::KIND
            && out_info.planes()? == planes
            && out_info.dimensions()? == dims
            && dims.len() >= 2
            && dims[1] == 2;
        let width = dims[0] as usize;
        let mut slices = cells.chunks((2 * width * planes).max(1));
        // SAFETY: the output belongs to this matrix_calc call; the input was copied.
        for mut chunk in unsafe { data.chunks_mut::<T>()? } {
            let mut entries: Vec<&mut [T]> = chunk.entry_iter_mut().collect();
            for cell in &mut entries {
                cell.fill(T::default());
            }
            let Some(slice) = slices.next().filter(|_| shaped) else {
                continue;
            };
            let (out0, out1) = entries.split_at_mut(width);
            let (in0, in1) = slice.split_at(width * planes);
            let mut state = self.next_seed();
            for j in 0..width {
                let a = &in0[j * planes..][..planes];
                let a2 = &in1[j * planes..][..planes];
                if a[0] == T::default() {
                    break;
                }
                particle(&p, &mut state, a, a2, &mut *out0[j], &mut *out1[j]);
            }
        }
        if shaped {
            Ok(())
        } else if out_info.kind()? != T::KIND || out_info.planes()? != planes {
            Err(err::JIT_ERR_MISMATCH_PLANE)
        } else {
            Err(err::JIT_ERR_MISMATCH_DIM)
        }
    }
}

/// Bounds one particle: `a`/`a2` are its current and previous cells, `o`/`o2` the output's.
fn particle<T: Real>(p: &Params<T>, state: &mut u32, a: &[T], a2: &[T], o: &mut [T], o2: &mut [T]) {
    let coordinates = a.len() - 2;
    o[..2].copy_from_slice(&a[..2]);
    o2[..2].copy_from_slice(&a2[..2]);
    let mut squish = [T::default(); SLOTS];
    if !matches!(p.mode, 1..=3) {
        for (k, s) in squish.iter_mut().enumerate().take(coordinates) {
            *s = p.squish[k] + p.squish_var[k] * T::of(random(state) as f64);
        }
    }
    let mut kill = false;
    for k in 0..coordinates {
        let (x, prev) = (a[k + 2], a2[k + 2]);
        let (lo, hi, elo, ehi) = (p.lo[k], p.hi[k], p.enable_lo[k], p.enable_hi[k]);
        let (out, out2) = match p.mode {
            // clip
            3 if elo && x < lo => (lo, prev),
            3 if ehi && x > hi => (hi, prev),
            // torus
            2 if ehi && x > hi && elo => (lo + (x - prev), lo),
            2 if elo && x < lo && ehi => (hi + (x - prev), hi),
            2 if (ehi && x > hi) || (elo && x < lo) => {
                kill = true;
                (x, prev)
            }
            // kill
            1 if elo && x < lo => {
                kill = true;
                (lo, prev)
            }
            1 if ehi && x > hi => {
                kill = true;
                (hi, prev)
            }
            1..=3 => (x, prev),
            // bounce
            _ if elo && ehi && (x < lo || x > hi) => {
                let t = clamp(x, lo, hi);
                (clamp(t - (t - prev) * squish[k], lo, hi), t)
            }
            _ if elo && x < lo => {
                let t = max(x, lo);
                (max(t - (t - prev) * squish[k], lo), t)
            }
            _ if ehi && x > hi => {
                let t = min(x, hi);
                (min(t - (t - prev) * squish[k], hi), t)
            }
            _ => (x, prev),
        };
        o[k + 2] = out;
        o2[k + 2] = out2;
    }
    if kill {
        o[1] = T::default();
    }
}

/// Writes `atoms` to a getter's out-parameters, allocating as Max expects if they are too small.
///
/// # Safety
/// `ac` and `av` must be the out-parameters Max passes an attribute getter.
unsafe fn write_atoms(
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
    atoms: Vec<Atom>,
) -> t_max_err {
    if ac.is_null() || av.is_null() {
        return err::JIT_ERR_NONE as _;
    }
    // SAFETY: the caller's contract; `*av` is null or holds `*ac` atoms, and a new array is the
    // caller's to free.
    unsafe {
        if (*av).is_null() || (*ac as usize) < atoms.len() {
            let size = atoms.len().max(1) * std::mem::size_of::<max_sys::t_atom>();
            let p = max_sys::jit_getbytes(size as c_long).cast::<max_sys::t_atom>();
            if p.is_null() {
                *ac = 0;
                return err::JIT_ERR_OUT_OF_MEM as _;
            }
            *av = p;
        }
        *ac = atoms.len() as c_long;
        for (i, atom) in atoms.into_iter().enumerate() {
            (*av).add(i).cast::<Atom>().write(atom);
        }
    }
    err::JIT_ERR_NONE as _
}

/// The setter's atoms, at most `SLOTS` of them.
///
/// # Safety
/// `av` must hold `ac` atoms, as Max passes a setter.
unsafe fn setter_atoms<'a>(ac: c_long, av: *mut max_sys::t_atom) -> &'a [Atom] {
    if ac > 0 && !av.is_null() {
        // SAFETY: the caller's contract; `Atom` is a transparent `t_atom`.
        unsafe { std::slice::from_raw_parts(av.cast(), (ac as usize).min(SLOTS)) }
    } else {
        &[]
    }
}

/// The getter of `bounds_hi` (`I` = `HI`) or `bounds_lo`: a float per set bound, `x` if disabled.
///
/// # Safety
/// Only Max calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn bounds_get<const I: usize>(
    w: &Core<PBounds, MatrixKind>,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "bounds getter", err::JIT_ERR_GENERIC as _, || {
        let b = *w.wrapped().bounds[I].lock().unwrap();
        let atoms = (0..b.count)
            .map(|i| {
                if b.enabled[i] {
                    b.values[i].into()
                } else {
                    sym!("x").into()
                }
            })
            .collect();
        // SAFETY: the out-parameters Max passed this getter.
        unsafe { write_atoms(ac, av, atoms) }
    })
}

/// The setter of `bounds_hi` or `bounds_lo`: a symbol disables a bound, a number sets and enables
/// it. Bounds past those given keep their state.
///
/// # Safety
/// Only Max calls it, as an attribute setter, with `av` holding `ac` atoms.
unsafe extern "C" fn bounds_set<const I: usize>(
    w: &Core<PBounds, MatrixKind>,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "bounds setter", err::JIT_ERR_GENERIC as _, || {
        // SAFETY: the atoms Max passed this setter.
        let atoms = unsafe { setter_atoms(ac, av) };
        let mut b = w.wrapped().bounds[I].lock().unwrap();
        for (i, atom) in atoms.iter().enumerate() {
            if matches!(atom.get_type(), Some(AtomType::Symbol)) {
                b.enabled[i] = false;
            } else {
                b.values[i] = atom.get_float();
                b.enabled[i] = true;
            }
        }
        b.count = atoms.len();
        err::JIT_ERR_NONE as _
    })
}

/// The getter of `squish` (`I` = `SQUISH`) or `squish_var`: the values set.
///
/// # Safety
/// Only Max calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn squish_get<const I: usize>(
    w: &Core<PBounds, MatrixKind>,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "squish getter", err::JIT_ERR_GENERIC as _, || {
        let a = *w.wrapped().squish[I].lock().unwrap();
        let atoms = a.values[..a.count].iter().map(Atom::from).collect();
        // SAFETY: the out-parameters Max passed this getter.
        unsafe { write_atoms(ac, av, atoms) }
    })
}

/// The setter of `squish` or `squish_var`.
///
/// # Safety
/// Only Max calls it, as an attribute setter, with `av` holding `ac` atoms.
unsafe extern "C" fn squish_set<const I: usize>(
    w: &Core<PBounds, MatrixKind>,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "squish setter", err::JIT_ERR_GENERIC as _, || {
        // SAFETY: the atoms Max passed this setter.
        let atoms = unsafe { setter_atoms(ac, av) };
        let mut a = w.wrapped().squish[I].lock().unwrap();
        for (value, atom) in a.values.iter_mut().zip(atoms) {
            *value = atom.get_float();
        }
        a.count = atoms.len();
        err::JIT_ERR_NONE as _
    })
}

median::ext_main! { PBounds }

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use median::jit::matrix::{MatrixDataType, MatrixInfo};
    use median_mock::{
        Atom,
        jit::{MatrixFixture, MatrixList},
    };

    type Calc = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64;

    fn setup(k: &median_mock::Kernel, args: &[Atom]) -> (*mut c_void, *mut c_void, Calc) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.p.bounds", args);
        assert!(!x.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        (x, core, unsafe { std::mem::transmute::<usize, Calc>(f) })
    }

    fn f64s(values: &[f64]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    fn read_f64s(bytes: &[u8]) -> Vec<f64> {
        bytes
            .chunks(8)
            .map(|b| f64::from_ne_bytes(b.try_into().unwrap()))
            .collect()
    }

    fn particles(width: usize, values: &[f64]) -> MatrixFixture {
        let info = MatrixInfo::new(MatrixDataType::Float64, 3, &[width, 2]).unwrap();
        let mut m = MatrixFixture::new(*info.as_raw());
        m.write_bytes(&f64s(values));
        m
    }

    /// Runs one particle (id 1, life 5) at `x`, previously at `prev`; returns the output's
    /// current (life, x) and previous x.
    fn step(calc: Calc, core: *mut c_void, x: f64, prev: f64) -> (f64, f64, f64) {
        let input = particles(1, &[1.0, 5.0, x, 1.0, 5.0, prev]);
        let out = particles(1, &[9.0; 6]);
        {
            let ins = MatrixList::new(&[&input]);
            let outs = MatrixList::new(&[&out]);
            assert_eq!(unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }, 0);
        }
        let v = read_f64s(&out.bytes());
        assert_eq!((v[0], v[3], v[4]), (1.0, 1.0, 5.0));
        (v[1], v[2], v[5])
    }

    #[test]
    fn bounces_off_a_bound_damped_by_squish() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &["@bounds_hi".into(), 1.0.into()]);
        assert_eq!(step(calc, core, 1.5, 0.5), (5.0, 0.5, 1.0));
        assert_eq!(step(calc, core, 0.5, 0.0), (5.0, 0.5, 0.0));
        k.attr_set(x, "squish", &[Atom::Float(0.5)]).unwrap();
        assert_eq!(k.attr_get(x, "squish").unwrap(), [Atom::Float(0.5)]);
        assert_eq!(step(calc, core, 1.5, 0.5), (5.0, 0.75, 1.0));
        // Both bounds: clamped into the range.
        k.attr_set(x, "bounds_lo", &[Atom::Float(0.0)]).unwrap();
        assert_eq!(step(calc, core, -1.0, 0.5), (5.0, 0.25, 0.0));
        assert!(k.free_object(x));
    }

    #[test]
    fn kill_torus_and_clip_modes() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &["@bounds_lo".into(), (-1.0).into()]);
        k.attr_set(x, "mode", &[Atom::Long(1)]).unwrap();
        assert_eq!(step(calc, core, -2.0, -0.5), (0.0, -1.0, -0.5));
        // Torus with only a low bound kills.
        k.attr_set(x, "mode", &[Atom::Long(2)]).unwrap();
        assert_eq!(step(calc, core, -2.0, -0.5), (0.0, -2.0, -0.5));
        k.attr_set(x, "bounds_hi", &[Atom::Float(1.0)]).unwrap();
        assert_eq!(step(calc, core, 1.5, 1.25), (5.0, -0.75, -1.0));
        assert_eq!(step(calc, core, -1.5, -1.25), (5.0, 0.75, 1.0));
        k.attr_set(x, "mode", &[Atom::Long(3)]).unwrap();
        assert_eq!(step(calc, core, -2.0, -0.5), (5.0, -1.0, -0.5));
        assert_eq!(step(calc, core, 2.0, 0.5), (5.0, 1.0, 0.5));
        assert!(k.free_object(x));
    }

    #[test]
    fn stops_at_the_first_dead_particle_and_clears_the_rest() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &[]);
        let input = particles(
            3,
            &[
                1.0, 5.0, 0.5, 0.0, 9.0, 9.0, 2.0, 7.0, 0.2, //
                1.0, 5.0, 0.4, 0.0, 9.0, 9.0, 2.0, 7.0, 0.1,
            ],
        );
        let out = particles(3, &[9.0; 18]);
        {
            let ins = MatrixList::new(&[&input]);
            let outs = MatrixList::new(&[&out]);
            assert_eq!(unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }, 0);
        }
        let mut expected = vec![0.0; 18];
        expected[..3].copy_from_slice(&[1.0, 5.0, 0.5]);
        expected[9..12].copy_from_slice(&[1.0, 5.0, 0.4]);
        assert_eq!(read_f64s(&out.bytes()), expected);
        assert_eq!((input.lock_state(), out.lock_state()), (0, 0));
        assert!(k.free_object(x));
    }

    #[test]
    fn bounds_report_disabled_entries_as_x() {
        let k = median_mock::lock();
        let (x, _, _) = setup(&k, &[]);
        assert!(k.attr_get(x, "bounds_hi").unwrap().is_empty());
        k.attr_set(
            x,
            "bounds_hi",
            &[Atom::Float(1.0), Atom::Sym("x".into()), Atom::Long(2)],
        )
        .unwrap();
        assert_eq!(
            k.attr_get(x, "bounds_hi").unwrap(),
            [Atom::Float(1.0), Atom::Sym("x".into()), Atom::Float(2.0)]
        );
        k.attr_set(x, "squish_var", &vec![Atom::Float(0.1); 40])
            .unwrap();
        assert_eq!(k.attr_get(x, "squish_var").unwrap().len(), 30);
        assert!(k.free_object(x));
    }

    #[test]
    fn squish_var_varies_the_bounce_within_range() {
        let k = median_mock::lock();
        let args = [
            "@bounds_hi".into(),
            1.0.into(),
            "@squish_var".into(),
            0.5.into(),
        ];
        let (x, core, calc) = setup(&k, &args);
        let mut seen = Vec::new();
        for _ in 0..8 {
            // Squish is 1 +- 0.5, so the bounce lands 0.25..0.75 below the bound.
            let (_, current, _) = step(calc, core, 1.5, 0.5);
            assert!((0.25..=0.75).contains(&current), "{current}");
            seen.push(current);
        }
        assert!(seen.windows(2).any(|w| w[0] != w[1]));
        assert!(k.free_object(x));
    }

    #[test]
    fn refuses_wrong_shapes_and_types() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &[]);
        let run = |input: &MatrixFixture, out: &MatrixFixture| {
            let ins = MatrixList::new(&[input]);
            let outs = MatrixList::new(&[out]);
            unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
        };
        let info = |kind, planes, dims: &[usize]| {
            MatrixFixture::new(*MatrixInfo::new(kind, planes, dims).unwrap().as_raw())
        };
        // Three rows: cleared, not processed.
        let rows3 = info(MatrixDataType::Float32, 3, &[1, 3]);
        assert_ne!(run(&rows3, &rows3), 0);
        let chars = info(MatrixDataType::Char, 3, &[1, 2]);
        let mut out = info(MatrixDataType::Char, 3, &[1, 2]);
        out.write_bytes(&[9; 6]);
        assert_ne!(run(&chars, &out), 0);
        assert_eq!(out.bytes(), [0; 6]);
        // The mock does not resize: the raw `dim` call is logged and the output refused.
        k.take_console_log();
        let wide = particles(2, &[0.0; 12]);
        let narrow = particles(1, &[0.0; 6]);
        assert_ne!(run(&wide, &narrow), 0);
        assert!(
            k.take_console_log()
                .iter()
                .any(|e| e.text.contains("unsupported Jitter method dim"))
        );
        assert!(k.free_object(x));
    }
}
