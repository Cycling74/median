//! A port of the SDK's `matrix/jit.findbounds`: reports the bounding box of the cells whose
//! planes all lie within `min` and `max`.
//!
//! The C replaces the MOP's matrix processing (`max_jit_classex_mop_mproc`) to output the bounds
//! after `matrix_calc`; median's Max side has no such hook, so `calc` sends them itself, on the
//! thread the matrix arrived on, as the C's processing step does. Deviations:
//! - the bounds come out of outlets 1 (min) and 2 (max), right of dumpout (outlet 0); the C puts
//!   them left of it;
//! - `min`, `max`, `boundmin` and `boundmax` are attributes with accessors written here (median has
//!   no array attributes), without `basic`;
//! - the C's bounds are only right for 1 and 2 dimensions (a higher dimension's `boundmax` stays
//!   -1 when one slice matches, and dimensions 0 and 1 keep only the last matching slice); the
//!   port computes the box over every dimension;
//! - a plane beyond the values given to `min` or `max` uses 0 (the C reads one value past them);
//! - the accessors run on the caller's thread (the C's defer them, `JIT_ATTR_GET_DEFER_LOW`,
//!   `JIT_ATTR_SET_USURP_LOW`).
//!
//! `calc` is tested by calling it directly; the `jit_matrix` path in Max is untested.

use std::{
    ffi::{c_long, c_void},
    sync::Mutex,
};

use median::{
    attr::{AttrBuilder, AttrTrampGetMethod, AttrTrampSetMethod, AttrType},
    error::MaxError,
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{
            Count, IOCount, JIT_MATRIX_MAX_PLANECOUNT, Matrix, MatrixData, MatrixDataType,
            iter::JitEntryType,
        },
        wrapper::{Core, MatrixKind},
    },
    max_sys::{self, t_jit_error_code as err, t_max_err},
    prelude::*,
    wrapper::WrapperWrapped,
};

/// A plane threshold array: `count` values in use.
#[derive(Clone, Copy)]
struct Limits {
    values: [f64; JIT_MATRIX_MAX_PLANECOUNT],
    count: usize,
}

/// Per dimension of the last matrix, -1 where no cell matched.
#[derive(Clone, Default)]
struct Bounds {
    min: Vec<t_atom_long>,
    max: Vec<t_atom_long>,
}

struct Outlets {
    min: Outlet<Any>,
    max: Outlet<Any>,
}

#[derive(JitMatrixObj)]
#[max(name = "jit.median.findbounds", no_methods)]
pub struct JitFindbounds {
    min: Mutex<Limits>,
    max: Mutex<Limits>,
    bounds: Mutex<Bounds>,
    /// `None` for a core made without a Max object (from JavaScript, say).
    outlets: Option<Outlets>,
}

impl MatrixOp for JitFindbounds {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        let outlets = match b.outlet(Outlet::any().assist("(list) min bounds")) {
            Ok(min) => Some(Outlets {
                min,
                max: b.outlet(Outlet::any().assist("(list) max bounds"))?,
            }),
            Err(_) => None,
        };
        let limits = Limits {
            values: [0.0; JIT_MATRIX_MAX_PLANECOUNT],
            count: 4,
        };
        Ok(Self {
            min: Mutex::new(limits),
            max: Mutex::new(limits),
            bounds: Mutex::new(Bounds::default()),
            outlets,
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(0),
        }
    }

    fn class_setup(class: &mut Class<Core<Self, MatrixKind>>) -> MaxResult<()> {
        type Get = AttrTrampGetMethod<Core<JitFindbounds, MatrixKind>>;
        type Set = AttrTrampSetMethod<Core<JitFindbounds, MatrixKind>>;
        let limits: [(&str, &'static std::ffi::CStr, Get, Set); 2] = [
            ("min", c"Minimum", get_limits::<false>, set_limits::<false>),
            ("max", c"Maximum", get_limits::<true>, set_limits::<true>),
        ];
        for (name, label, get, set) in limits {
            // GAP: CLASS_ATTR_SYM_VARSIZE (array attributes)
            let mut b = AttrBuilder::new_accessors(name, AttrType::Float64, get, set);
            b.label(label).category(c"Behavior");
            class.add_attribute(b.build().map_err(|_| MaxError::Generic)?)?;
        }
        let bounds: [(&str, &'static std::ffi::CStr, Get); 2] = [
            ("boundmin", c"Bounding Box Minimum", get_bounds::<false>),
            ("boundmax", c"Bounding Box Maximum", get_bounds::<true>),
        ];
        for (name, label, get) in bounds {
            // GAP: CLASS_ATTR_SYM_VARSIZE (array attributes)
            let mut b = AttrBuilder::new_get(name, AttrType::Int64, get);
            b.label(label).category(c"Value");
            class.add_attribute(b.build().map_err(|_| MaxError::Generic)?)?;
        }
        Ok(())
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        let input = inputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let (min, max) = (*self.min.lock().unwrap(), *self.max.lock().unwrap());
        let bounds = {
            let mut lock = input.lock();
            let data = lock.data().map_err(|_| err::JIT_ERR_INVALID_INPUT)?;
            let kind = data.info().kind()?;
            let planes = data.info().planes()?;
            // The thresholds in the matrix's type, as the C converts them.
            let limit = |l: &Limits, p: usize| {
                let v = if p < l.count { l.values[p] } else { 0.0 };
                match kind {
                    MatrixDataType::Char => (v * 255.0).clamp(0.0, 255.0).trunc(),
                    MatrixDataType::Int32 => v.trunc(),
                    MatrixDataType::Float32 => v as f32 as f64,
                    MatrixDataType::Float64 => v,
                }
            };
            let lo: Vec<f64> = (0..planes).map(|p| limit(&min, p)).collect();
            let hi: Vec<f64> = (0..planes).map(|p| limit(&max, p)).collect();
            match kind {
                MatrixDataType::Char => bounds::<u8>(&data, &lo, &hi)?,
                MatrixDataType::Int32 => bounds::<i32>(&data, &lo, &hi)?,
                MatrixDataType::Float32 => bounds::<f32>(&data, &lo, &hi)?,
                MatrixDataType::Float64 => bounds::<f64>(&data, &lo, &hi)?,
            }
        };
        *self.bounds.lock().unwrap() = bounds.clone();
        if let Some(o) = &self.outlets {
            send(&o.max, &bounds.max);
            send(&o.min, &bounds.min);
        }
        Ok(())
    }
}

/// The bounding box of the cells of `data` whose every plane `p` is within `lo[p]..=hi[p]`.
fn bounds<T: JitEntryType + Into<f64>>(
    data: &MatrixData<'_>,
    lo: &[f64],
    hi: &[f64],
) -> JitResult<Bounds> {
    let dims: Vec<usize> = data
        .info()
        .dimensions()?
        .iter()
        .map(|&d| d as usize)
        .collect();
    let mut b = Bounds {
        min: vec![-1; dims.len()],
        max: vec![-1; dims.len()],
    };
    let mut include = |d: usize, i: usize| {
        let i = i as t_atom_long;
        if b.min[d] < 0 || i < b.min[d] {
            b.min[d] = i;
        }
        b.max[d] = b.max[d].max(i);
    };
    // SAFETY: the input stays locked and unwritten for this matrix_calc call.
    for (index, chunk) in unsafe { data.chunks::<T>()? }.enumerate() {
        let mut any = false;
        for (cell_index, cell) in chunk.entry_iter().enumerate() {
            let inside = cell.iter().zip(lo.iter().zip(hi)).all(|(&v, (&lo, &hi))| {
                let v: f64 = v.into();
                v >= lo && v <= hi
            });
            if inside {
                any = true;
                include(0, cell_index % dims[0]);
                if dims.len() > 1 {
                    include(1, cell_index / dims[0]);
                }
            }
        }
        if any {
            // Chunks run over dimensions 2 and up, dimension 2 fastest.
            let mut rest = index;
            for (d, &n) in dims.iter().enumerate().skip(2) {
                include(d, rest % n);
                rest /= n;
            }
        }
    }
    Ok(b)
}

fn send(outlet: &Outlet<Any>, values: &[t_atom_long]) {
    let _ = match values {
        [v] => outlet.send_float(*v as f64),
        _ => {
            let atoms: Vec<Atom> = values.iter().map(|&v| v.into()).collect();
            outlet.send(&sym!("list"), &atoms)
        }
    };
}

/// The getter of `max` (`MAX`) or `min`.
///
/// # Safety
/// Only Jitter calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn get_limits<const MAX: bool>(
    w: &Core<JitFindbounds, MatrixKind>,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "findbounds getter", err::JIT_ERR_GENERIC as _, || {
        let s = w.wrapped();
        let l = *if MAX { &s.max } else { &s.min }.lock().unwrap();
        let atoms = l.values[..l.count].iter().map(|&v| v.into()).collect();
        // SAFETY: the getter's own `ac` and `av`
        unsafe { write_atoms(ac, av, atoms) }
    })
}

/// The setter of `max` (`MAX`) or `min`: the first values are replaced, as many as are given.
///
/// # Safety
/// Only Jitter calls it, as an attribute setter, with `av` holding `ac` atoms.
unsafe extern "C" fn set_limits<const MAX: bool>(
    w: &Core<JitFindbounds, MatrixKind>,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "findbounds setter", err::JIT_ERR_GENERIC as _, || {
        let atoms: &[Atom] = if ac > 0 && !av.is_null() {
            // SAFETY: Jitter passes `ac` valid atoms; `Atom` is a transparent `t_atom`.
            unsafe { std::slice::from_raw_parts(av.cast(), ac as usize) }
        } else {
            &[]
        };
        let s = w.wrapped();
        let mut l = if MAX { &s.max } else { &s.min }.lock().unwrap();
        l.count = atoms.len().min(JIT_MATRIX_MAX_PLANECOUNT);
        for (v, a) in l.values.iter_mut().zip(atoms) {
            *v = a.get_float();
        }
        0
    })
}

/// The getter of `boundmax` (`MAX`) or `boundmin`.
///
/// # Safety
/// Only Jitter calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn get_bounds<const MAX: bool>(
    w: &Core<JitFindbounds, MatrixKind>,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "findbounds getter", err::JIT_ERR_GENERIC as _, || {
        let b = w.wrapped().bounds.lock().unwrap().clone();
        let values = if MAX { b.max } else { b.min };
        // SAFETY: the getter's own `ac` and `av`
        unsafe { write_atoms(ac, av, values.into_iter().map(Atom::from).collect()) }
    })
}

/// Fill a Jitter getter's results: as many atoms as fit into an array the caller passes, or all
/// of them into a new one the caller frees.
///
/// # Safety
/// `ac` and `av` are null or the out-parameters Jitter passes to an attribute getter; a non-null
/// `*av` holds `*ac` atoms.
unsafe fn write_atoms(
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
    atoms: Vec<Atom>,
) -> t_max_err {
    if ac.is_null() || av.is_null() {
        return 0;
    }
    // SAFETY: the caller's contract; `Atom` is a transparent `t_atom`
    unsafe {
        if (*av).is_null() || *ac <= 0 {
            let size = atoms.len().max(1) * std::mem::size_of::<max_sys::t_atom>();
            // GAP: Custom attribute accessors
            let p = max_sys::jit_getbytes(size as c_long).cast::<max_sys::t_atom>();
            if p.is_null() {
                *ac = 0;
                return err::JIT_ERR_OUT_OF_MEM as _;
            }
            *av = p;
            *ac = atoms.len() as c_long;
        } else {
            *ac = (*ac).min(atoms.len() as c_long);
        }
        for (i, atom) in atoms.into_iter().take(*ac as usize).enumerate() {
            (*av).add(i).cast::<Atom>().write(atom);
        }
    }
    0
}

median::ext_main! { JitFindbounds }

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use median::jit::matrix::{MatrixDataType, MatrixInfo};
    use median_mock::{
        Atom,
        jit::{MatrixFixture, MatrixList},
    };

    type Calc =
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> median::max_sys::t_jit_err;

    fn setup(k: &median_mock::Kernel, args: &[Atom]) -> (*mut c_void, *mut c_void, Calc) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.findbounds", args);
        assert!(!x.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        // SAFETY: `matrix_calc` is registered with this signature
        (x, core, unsafe { std::mem::transmute::<usize, Calc>(f) })
    }

    fn run(calc: Calc, core: *mut c_void, m: &MatrixFixture) -> median::max_sys::t_jit_err {
        let ins = MatrixList::new(&[m]);
        let outs = MatrixList::new(&[]);
        // SAFETY: live core and lists
        unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
    }

    fn fixture(kind: MatrixDataType, planes: usize, dims: &[usize], bytes: &[u8]) -> MatrixFixture {
        let info = MatrixInfo::new(kind, planes, dims).unwrap();
        let mut m = MatrixFixture::new(*info.as_raw());
        m.write_bytes(bytes);
        m
    }

    fn outputs(k: &median_mock::Kernel) -> Vec<(usize, String, Vec<Atom>)> {
        k.take_outlet_log()
            .into_iter()
            .map(|e| (e.index, e.selector, e.atoms))
            .collect()
    }

    fn longs(v: &[i64]) -> Vec<Atom> {
        v.iter().map(|&v| Atom::Long(v)).collect()
    }

    #[test]
    fn char_bounds_use_scaled_limits_and_go_out_max_then_min() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &["@min".into(), 0.5.into(), "@max".into(), 1.0.into()]);
        assert_eq!(k.assist(x, true, 1).unwrap(), "(list) min bounds");
        assert_eq!(k.assist(x, true, 2).unwrap(), "(list) max bounds");
        assert_eq!(k.attr_get(x, "min").unwrap(), [Atom::Float(0.5)]);
        #[rustfmt::skip]
        let m = fixture(MatrixDataType::Char, 1, &[4, 3], &[
            0,   0,   0, 0,
            0, 200, 127, 0,
            0, 128,   0, 0,
        ]);
        assert_eq!(run(calc, core, &m), 0);
        assert_eq!(
            outputs(&k),
            [
                (2, "list".into(), longs(&[2, 2])),
                (1, "list".into(), longs(&[1, 1])),
            ]
        );
        assert_eq!(k.attr_get(x, "boundmax").unwrap(), longs(&[2, 2]));
        assert_eq!(k.attr_get(x, "boundmin").unwrap(), longs(&[1, 1]));
        assert!(k.free_object(x));
    }

    #[test]
    fn every_plane_must_match_and_unset_planes_use_zero() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &[]);
        k.attr_set(x, "min", &[Atom::Float(1.0)]).unwrap();
        k.attr_set(x, "max", &[Atom::Float(5.0)]).unwrap();
        let bytes: Vec<u8> = [3i32, 0, 3, 1, 9, 0, 1, 0]
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        let m = fixture(MatrixDataType::Int32, 2, &[4], &bytes);
        assert_eq!(run(calc, core, &m), 0);
        // Cells 0 and 3 match; a single dimension goes out as a float.
        assert_eq!(
            outputs(&k),
            [
                (2, "float".into(), vec![Atom::Float(3.0)]),
                (1, "float".into(), vec![Atom::Float(0.0)]),
            ]
        );
        let m = fixture(MatrixDataType::Int32, 2, &[2], &[0; 16]);
        assert_eq!(run(calc, core, &m), 0);
        assert_eq!(
            outputs(&k),
            [
                (2, "float".into(), vec![Atom::Float(-1.0)]),
                (1, "float".into(), vec![Atom::Float(-1.0)]),
            ]
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn three_dimensions_bound_every_dimension() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &["@min".into(), 0.5.into(), "@max".into(), 1.0.into()]);
        let mut cells = [0f32; 2 * 2 * 3];
        cells[1] = 0.7; // (1, 0, 0)
        cells[2 * 2 * 2 + 2] = 0.9; // (0, 1, 2)
        let bytes: Vec<u8> = cells.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let m = fixture(MatrixDataType::Float32, 1, &[2, 2, 3], &bytes);
        assert_eq!(run(calc, core, &m), 0);
        assert_eq!(
            outputs(&k),
            [
                (2, "list".into(), longs(&[1, 1, 2])),
                (1, "list".into(), longs(&[0, 0, 0])),
            ]
        );
        assert!(k.free_object(x));
    }
}
