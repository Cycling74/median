//! A port of the SDK's `matrix/jit.gradient`: fills a 4-plane char matrix with a gradient from
//! `start` to `end` along the first dimension, shaped by Chebyshev coefficients (`cheby`).
//!
//! `start`, `end` and `cheby` are float64 arrays (`jit_attr_offset_array` in the C), registered
//! here with hand-written accessors. They are not basic attributes as in the C.
//! Deviations: the C's Max wrapper passes `MAX_JIT_MOP_FLAGS_OWN_JIT_MATRIX` and
//! `MAX_JIT_MOP_FLAGS_OWN_OUTPUTMATRIX`, so a matrix arriving at its inlet is ignored; median's
//! Max side uses the default MOP behaviour, so here a matrix at the inlet sets the output's size
//! (with `adapt`) and outputs the gradient. Every row is the first row (the C copies 4 bytes per
//! cell, which is the same for its 4 planes), and an output that is not char is an error.

use core::ffi::{c_long, c_void};
use median::{
    attr::{AttrBuilder, AttrTrampGetMethod, AttrTrampSetMethod, AttrType},
    error::MaxError,
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix, MatrixDataType},
        wrapper::{Core, MatrixKind},
    },
    max_sys::{self, t_jit_error_code as err, t_max_err},
    prelude::*,
};
use std::{ffi::CStr, sync::Mutex};

const START: usize = 0;
const END: usize = 1;
const CHEBY: usize = 2;

/// An array attribute: `values` has the attribute's capacity, `count` of them were set.
struct FloatArray {
    values: Vec<f64>,
    count: usize,
}

impl FloatArray {
    fn new(values: Vec<f64>, count: usize) -> Mutex<Self> {
        Mutex::new(Self { values, count })
    }
}

#[derive(JitMatrixObj)]
#[max(name = "jit.median.gradient", no_methods)]
pub struct Gradient {
    /// Indexed by `START`, `END` and `CHEBY`.
    arrays: [Mutex<FloatArray>; 3],
}

type Getter = AttrTrampGetMethod<Core<Gradient, MatrixKind>>;
type Setter = AttrTrampSetMethod<Core<Gradient, MatrixKind>>;

impl MatrixOp for Gradient {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        let mut cheby = vec![0.0; 64];
        cheby[0] = 1.0;
        Ok(Self {
            arrays: [
                FloatArray::new(vec![0.0; 4], 4),
                FloatArray::new(vec![1.0; 4], 4),
                FloatArray::new(cheby, 1),
            ],
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }

    fn mop_setup(mop: &mut MOP<'_>) -> JitResult<()> {
        mop.single_type(MatrixDataType::Char)?;
        mop.single_plane_count(4)
    }

    fn class_setup(class: &mut Class<Core<Self, MatrixKind>>) -> MaxResult<()> {
        let arrays: [(&str, &'static CStr, Getter, Setter); 3] = [
            ("start", c"Start", array_get::<START>, array_set::<START>),
            ("end", c"End", array_get::<END>, array_set::<END>),
            (
                "cheby",
                c"Chebyshev Coefficients",
                array_get::<CHEBY>,
                array_set::<CHEBY>,
            ),
        ];
        for (name, label, get, set) in arrays {
            // GAP: CLASS_ATTR_SYM_VARSIZE (array attributes)
            let mut b = AttrBuilder::new_accessors(name, AttrType::Float64, get, set);
            b.label(label).category(c"Behavior");
            class.add_attribute(b.build().map_err(|_| MaxError::Generic)?)?;
        }
        Ok(())
    }

    fn calc(&self, _: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let ends = |i: usize| -> [i64; 4] {
            let a = self.arrays[i].lock().unwrap();
            std::array::from_fn(|k| (a.values[k] * 255.0) as i64)
        };
        let (start, end) = (ends(START), ends(END));
        let cheby = {
            let a = self.arrays[CHEBY].lock().unwrap();
            a.values[..a.count].to_vec()
        };
        let mut lock = output.lock();
        let mut data = lock.data().map_err(|_| err::JIT_ERR_INVALID_OUTPUT)?;
        let info = *data.info();
        if info.kind()? != MatrixDataType::Char {
            return Err(err::JIT_ERR_MISMATCH_TYPE);
        }
        let width = info.dimensions()?[0] as usize;
        let table = transfer(width, &cheby);
        let row: Vec<[u8; 4]> = table
            .iter()
            .map(|&t| {
                std::array::from_fn(|k| {
                    let v = if start[k] < end[k] {
                        t * (end[k] - start[k]) as f64 + start[k] as f64
                    } else {
                        (1.0 - t) * (start[k] - end[k]) as f64 + end[k] as f64
                    };
                    v as u8
                })
            })
            .collect();
        // SAFETY: the output belongs to this matrix_calc call and nothing else borrows it.
        for mut chunk in unsafe { data.chunks_mut::<u8>()? } {
            for (i, cell) in chunk.entry_iter_mut().enumerate() {
                let planes = cell.len().min(4);
                cell[..planes].copy_from_slice(&row[i % width][..planes]);
            }
        }
        Ok(())
    }
}

/// The Chebyshev transfer function over `width` cells, scaled to 0..1, with the C's mix of float
/// and double arithmetic.
fn transfer(width: usize, cheby: &[f64]) -> Vec<f64> {
    let d = ((width / 2) as f64 - 0.5) as f32;
    let mut xmax = 0.0f32;
    let mut table: Vec<f64> = (0..width)
        .map(|i| {
            let v = ((i as f32 / d) as f64 - 1.0) as f32;
            let (mut t, mut tn1, mut tn) = (0.0f64, 1.0f32, v);
            for &c in cheby {
                t += c * tn as f64;
                let tn2 = tn1;
                tn1 = tn;
                tn = 2.0 * v * tn1 - tn2;
            }
            let wmax = t.abs() as f32;
            if wmax > xmax {
                xmax = wmax;
            }
            t
        })
        .collect();
    for t in &mut table {
        *t = (*t / xmax as f64 + 1.0) / 2.0;
    }
    table
}

/// The getter of the array attribute `I`: its set values.
///
/// # Safety
/// Only Max calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn array_get<const I: usize>(
    w: &Core<Gradient, MatrixKind>,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "array getter", err::JIT_ERR_GENERIC as _, || {
        if ac.is_null() || av.is_null() {
            return err::JIT_ERR_NONE as _;
        }
        let values = {
            let a = w.wrapped().arrays[I].lock().unwrap();
            a.values[..a.count].to_vec()
        };
        // SAFETY: `ac` and `av` are the getter's out-parameters; `*av` is null or holds `*ac`
        // atoms, and a new array is the caller's to free.
        unsafe {
            if (*av).is_null() || (*ac as usize) < values.len() {
                let size = values.len().max(1) * std::mem::size_of::<max_sys::t_atom>();
                let p = max_sys::jit_getbytes(size as c_long).cast::<max_sys::t_atom>();
                if p.is_null() {
                    *ac = 0;
                    return err::JIT_ERR_OUT_OF_MEM as _;
                }
                *av = p;
            }
            *ac = values.len() as c_long;
            for (i, v) in values.into_iter().enumerate() {
                (*av).add(i).cast::<Atom>().write(Atom::from(v));
            }
        }
        err::JIT_ERR_NONE as _
    })
}

/// The setter of the array attribute `I`: sets as many values as it gets, up to its capacity.
///
/// # Safety
/// Only Max calls it, as an attribute setter, with `av` holding `ac` atoms.
unsafe extern "C" fn array_set<const I: usize>(
    w: &Core<Gradient, MatrixKind>,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "array setter", err::JIT_ERR_GENERIC as _, || {
        let atoms: &[Atom] = if ac > 0 && !av.is_null() {
            // SAFETY: Max passes `ac` valid atoms; `Atom` is a transparent `t_atom`.
            unsafe { std::slice::from_raw_parts(av.cast(), ac as usize) }
        } else {
            &[]
        };
        let mut a = w.wrapped().arrays[I].lock().unwrap();
        let count = atoms.len().min(a.values.len());
        for (value, atom) in a.values.iter_mut().zip(&atoms[..count]) {
            *value = atom.get_float();
        }
        a.count = count;
        err::JIT_ERR_NONE as _
    })
}

median::ext_main! { Gradient }

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
        let x = k.new_object("jit.median.gradient", args);
        assert!(!x.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        (x, core, unsafe { std::mem::transmute::<usize, Calc>(f) })
    }

    fn fill(calc: Calc, core: *mut c_void, kind: MatrixDataType, dims: &[usize]) -> (i64, Vec<u8>) {
        let info = MatrixInfo::new(kind, 4, dims).unwrap();
        let out = MatrixFixture::new(*info.as_raw());
        let input = MatrixFixture::new(*info.as_raw());
        let code = {
            let ins = MatrixList::new(&[&input]);
            let outs = MatrixList::new(&[&out]);
            unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
        };
        assert_eq!(out.lock_state(), 0);
        (code, out.bytes())
    }

    #[test]
    fn default_gradient_is_a_black_to_white_ramp_on_every_row() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &[]);
        let (code, bytes) = fill(calc, core, MatrixDataType::Char, &[4, 2]);
        assert_eq!(code, 0);
        let row = [
            0, 0, 0, 0, 85, 85, 85, 85, 170, 170, 170, 170, 255, 255, 255, 255,
        ];
        assert_eq!(bytes[..16], row);
        assert_eq!(bytes[16..], row);
        assert!(k.free_object(x));
    }

    #[test]
    fn start_above_end_runs_down_and_cheby_shapes_the_curve() {
        let k = median_mock::lock();
        let args = [
            "@start".into(),
            1.0.into(),
            0.0.into(),
            "@end".into(),
            0.0.into(),
        ];
        let (x, core, calc) = setup(&k, &args);
        assert_eq!(
            k.attr_get(x, "start").unwrap(),
            [Atom::Float(1.0), Atom::Float(0.0)]
        );
        // Values past those set keep their defaults: start 0 0 0 0, end 1 1 1 1. Falling planes
        // truncate (1 - t) * 255 just below 170 and 85.
        let (_, bytes) = fill(calc, core, MatrixDataType::Char, &[4]);
        assert_eq!(
            bytes,
            [
                255, 0, 0, 0, 169, 85, 85, 85, 84, 170, 170, 170, 0, 255, 255, 255
            ]
        );

        // The coefficients weight T1, T2, ...; T2(v) = 2v^2 - 1 over v = -1, -1/3, 1/3, 1 is
        // 1, -7/9, -7/9, 1.
        k.attr_set(x, "cheby", &[Atom::Float(0.0), Atom::Float(1.0)])
            .unwrap();
        assert_eq!(k.attr_get(x, "cheby").unwrap().len(), 2);
        k.attr_set(x, "start", &[Atom::Float(0.0)]).unwrap();
        k.attr_set(x, "end", &[Atom::Float(1.0)]).unwrap();
        let (_, bytes) = fill(calc, core, MatrixDataType::Char, &[4]);
        assert_eq!(
            bytes,
            [
                255, 255, 255, 255, 28, 28, 28, 28, 28, 28, 28, 28, 255, 255, 255, 255
            ]
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn arrays_keep_at_most_their_capacity_and_refuse_other_types() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &[]);
        assert_eq!(k.attr_get(x, "end").unwrap(), vec![Atom::Float(1.0); 4]);
        k.attr_set(x, "end", &vec![Atom::Float(0.5); 6]).unwrap();
        assert_eq!(k.attr_get(x, "end").unwrap(), vec![Atom::Float(0.5); 4]);
        let (_, label) = k.attr_attr(core, "cheby", "label").unwrap();
        assert_eq!(label, [Atom::Sym("Chebyshev Coefficients".into())]);
        let (code, _) = fill(calc, core, MatrixDataType::Float32, &[2]);
        assert_ne!(code, 0);
        assert!(k.free_object(x));
    }
}
