//! A port of the SDK's `matrix/jit.spill`: outputs one plane of a matrix's cells as a list,
//! `listlength` cells long, starting at the cell `offset` names.
//!
//! The C replaces the MOP's matrix processing (`max_jit_classex_mop_mproc`) to output the list
//! after `matrix_calc`; median's Max side has no such hook, so `calc` sends it itself, on the
//! thread the matrix arrived on, as the C's processing step does. Deviations:
//! - the list comes out of outlet 1, right of dumpout (outlet 0); the C puts it left of it;
//! - `plane` is an int attribute (median has no char attributes), clamped to the matrix's planes
//!   for every type (the C clamps it for char only and reads past the cell otherwise);
//! - `offset` is an attribute with accessors written here (median has no array attributes);
//! - the C's `spill` attribute, which users can neither get nor set, is a plain field;
//! - the accessors run on the caller's thread (the C's defer them, `JIT_ATTR_GET_DEFER_LOW`,
//!   `JIT_ATTR_SET_USURP_LOW`);
//! - that `#[bang]` replaces the MOP's own `bang` on the Max class is untested.
//!
//! `calc` is tested by calling it directly; the `jit_matrix` path in Max is untested.

use std::{
    ffi::{c_long, c_void},
    sync::Mutex,
};

use median::{
    attr::{AttrBuilder, AttrType},
    error::MaxError,
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{
            Count, IOCount, JIT_MATRIX_MAX_DIMCOUNT, Matrix, MatrixData, MatrixDataType,
            iter::JitEntryType,
        },
        wrapper::{Core, MatrixKind},
    },
    max_sys::{self, t_jit_error_code as err, t_max_err},
    prelude::*,
    wrapper::WrapperWrapped,
};

const MAX_OUT: t_atom_long = 16384;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Int(t_atom_long),
    Float(f64),
}

impl From<Value> for Atom {
    fn from(v: Value) -> Self {
        match v {
            Value::Int(i) => i.into(),
            Value::Float(f) => f.into(),
        }
    }
}

/// An offset per dimension; `count` of them are reported, all of them are used.
#[derive(Clone, Copy)]
struct Offset {
    values: [t_atom_long; JIT_MATRIX_MAX_DIMCOUNT],
    count: usize,
}

#[derive(JitMatrixObj)]
#[max(name = "jit.median.spill")]
pub struct JitSpill {
    #[attr(label = "List Length")]
    listlength: Int64,
    #[attr(label = "Plane")]
    plane: Int64,
    offset: Mutex<Offset>,
    spill: Mutex<Vec<Value>>,
    /// `None` for a core made without a Max object (from JavaScript, say).
    outlet: Option<Outlet<Any>>,
}

impl MatrixOp for JitSpill {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            listlength: Int64::new(256),
            plane: Int64::new(0),
            offset: Mutex::new(Offset {
                values: [0; JIT_MATRIX_MAX_DIMCOUNT],
                count: 2,
            }),
            spill: Mutex::new(Vec::new()),
            outlet: b.outlet(Outlet::any().assist("(list) spill")).ok(),
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(0),
        }
    }

    fn class_setup(class: &mut Class<Core<Self, MatrixKind>>) -> MaxResult<()> {
        // GAP: CLASS_ATTR_SYM_VARSIZE (array attributes)
        let mut b = AttrBuilder::new_accessors("offset", AttrType::Int64, get_offset, set_offset);
        b.label(c"Offset");
        class.add_attribute(b.build().map_err(|_| MaxError::Generic)?)
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        let input = inputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let offset = self.offset.lock().unwrap().values;
        let length = self.listlength.get().clamp(0, MAX_OUT) as usize;
        let spill = {
            let mut lock = input.lock();
            let data = lock.data().map_err(|_| err::JIT_ERR_INVALID_INPUT)?;
            let plane =
                self.plane
                    .get()
                    .clamp(0, data.info().planes()? as t_atom_long - 1) as usize;
            let spill = Spill {
                offset: &offset,
                plane,
                length,
            };
            match data.info().kind()? {
                MatrixDataType::Char => spill.run::<u8>(&data, |v| Value::Int(v as t_atom_long))?,
                MatrixDataType::Int32 => {
                    spill.run::<i32>(&data, |v| Value::Int(v as t_atom_long))?
                }
                MatrixDataType::Float32 => spill.run::<f32>(&data, |v| Value::Float(v.into()))?,
                MatrixDataType::Float64 => spill.run::<f64>(&data, Value::Float)?,
            }
        };
        *self.spill.lock().unwrap() = spill;
        self.output();
        Ok(())
    }
}

struct Spill<'a> {
    offset: &'a [t_atom_long; JIT_MATRIX_MAX_DIMCOUNT],
    plane: usize,
    length: usize,
}

impl Spill<'_> {
    /// Up to `length` values of `plane`, from the offset cell to the end of its 2D slice, row by
    /// row; the slice is the one the offsets of dimensions 2 and up select.
    fn run<T: JitEntryType>(
        &self,
        data: &MatrixData<'_>,
        value: impl Fn(T) -> Value,
    ) -> JitResult<Vec<Value>> {
        let dims = data.info().dimensions()?;
        let at = |d: usize| {
            let n = dims.get(d).map_or(1, |&n| n as t_atom_long);
            self.offset[d].clamp(0, n - 1) as usize
        };
        // Chunks run over dimensions 2 and up, dimension 2 fastest.
        let (mut slice, mut stride) = (0, 1);
        for (d, &n) in dims.iter().enumerate().skip(2) {
            slice += at(d) * stride;
            stride *= n as usize;
        }
        let width = dims[0] as usize;
        // SAFETY: the input stays locked and unwritten for this matrix_calc call.
        let Some(chunk) = (unsafe { data.chunks::<T>()? }).nth(slice) else {
            return Ok(Vec::new());
        };
        Ok(chunk
            .entry_iter()
            .skip(at(1) * width + at(0))
            .take(self.length)
            .map(|cell| value(cell[self.plane]))
            .collect())
    }
}

#[median::methods]
impl JitSpill {
    #[bang]
    pub fn output(&self) {
        let Some(outlet) = &self.outlet else { return };
        let spill = self.spill.lock().unwrap().clone();
        let _ = match spill[..] {
            [Value::Int(i)] => outlet.send_int(i),
            [Value::Float(f)] => outlet.send_float(f),
            _ => {
                let atoms: Vec<Atom> = spill.into_iter().map(Atom::from).collect();
                outlet.send(&sym!("list"), &atoms)
            }
        };
    }
}

/// The `offset` getter.
///
/// # Safety
/// Only Jitter calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn get_offset(
    w: &Core<JitSpill, MatrixKind>,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "offset getter", err::JIT_ERR_GENERIC as _, || {
        let o = *w.wrapped().offset.lock().unwrap();
        let atoms = o.values[..o.count].iter().map(Atom::from).collect();
        // SAFETY: the getter's own `ac` and `av`
        unsafe { write_atoms(ac, av, atoms) }
    })
}

/// The `offset` setter: the first offsets are replaced, as many as are given.
///
/// # Safety
/// Only Jitter calls it, as an attribute setter, with `av` holding `ac` atoms.
unsafe extern "C" fn set_offset(
    w: &Core<JitSpill, MatrixKind>,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "offset setter", err::JIT_ERR_GENERIC as _, || {
        let atoms: &[Atom] = if ac > 0 && !av.is_null() {
            // SAFETY: Jitter passes `ac` valid atoms; `Atom` is a transparent `t_atom`.
            unsafe { std::slice::from_raw_parts(av.cast(), ac as usize) }
        } else {
            &[]
        };
        let mut o = w.wrapped().offset.lock().unwrap();
        o.count = atoms.len().min(JIT_MATRIX_MAX_DIMCOUNT);
        for (v, a) in o.values.iter_mut().zip(atoms) {
            *v = a.get_int();
        }
        0
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

median::ext_main! { JitSpill }

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
        let x = k.new_object("jit.median.spill", args);
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
    fn spills_one_plane_from_the_offset_row_by_row() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(
            &k,
            &[
                "@plane".into(),
                1i64.into(),
                "@offset".into(),
                2i64.into(),
                0i64.into(),
                "@listlength".into(),
                4i64.into(),
            ],
        );
        assert_eq!(k.assist(x, true, 1).unwrap(), "(list) spill");
        assert_eq!(k.attr_get(x, "offset").unwrap(), longs(&[2, 0]));
        // Plane 1 of cell (x, y) is 10 * y + x.
        let bytes: Vec<u8> = (0..3u8)
            .flat_map(|y| (0..3u8).flat_map(move |x| [0, 10 * y + x]))
            .collect();
        let m = fixture(MatrixDataType::Char, 2, &[3, 3], &bytes);
        assert_eq!(run(calc, core, &m), 0);
        assert_eq!(outputs(&k), [(1, "list".into(), longs(&[2, 10, 11, 12]))]);
        k.send(x, "bang", &[]).unwrap();
        assert_eq!(outputs(&k), [(1, "list".into(), longs(&[2, 10, 11, 12]))]);

        // The list stops at the end of the slice; offsets and plane are clamped.
        k.attr_set(x, "offset", &[Atom::Long(1), Atom::Long(9)])
            .unwrap();
        k.attr_set(x, "plane", &[Atom::Long(7)]).unwrap();
        assert_eq!(run(calc, core, &m), 0);
        assert_eq!(outputs(&k), [(1, "list".into(), longs(&[21, 22]))]);
        assert!(k.free_object(x));
    }

    #[test]
    fn higher_offsets_pick_the_slice_and_one_value_is_a_scalar() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &[]);
        k.attr_set(x, "offset", &[Atom::Long(1), Atom::Long(0), Atom::Long(1)])
            .unwrap();
        k.attr_set(x, "listlength", &[Atom::Long(1)]).unwrap();
        let bytes: Vec<u8> = (0..8)
            .flat_map(|i| (i as f32 * 0.5).to_ne_bytes())
            .collect();
        let m = fixture(MatrixDataType::Float32, 1, &[2, 2, 2], &bytes);
        assert_eq!(run(calc, core, &m), 0);
        assert_eq!(outputs(&k), [(1, "float".into(), vec![Atom::Float(2.5)])]);

        k.attr_set(x, "listlength", &[Atom::Long(0)]).unwrap();
        assert_eq!(run(calc, core, &m), 0);
        assert_eq!(outputs(&k), [(1, "list".into(), vec![])]);
        assert!(k.free_object(x));
    }
}
