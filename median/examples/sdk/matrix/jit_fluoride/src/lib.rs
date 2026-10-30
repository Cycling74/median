//! A port of the SDK's `matrix/jit.fluoride`: a neon glow on a 4-plane char matrix. Cells whose
//! luminance approaches `lum` fade into the `glow` color, brighter ones fade to black, and cells
//! darker than `lum - tol` pass (as grey in `mode` 0, in color in `mode` 1).
//!
//! The C calculates with `jit_parallel_ndim_simplecalc2`; the port runs serially over median's
//! chunk views. `glow` is a three-element array attribute (`jit_attr_offset_array` in the C),
//! registered with accessors written here. The attributes are not basic.

use core::ffi::{c_long, c_void};
use median::{
    attr::{AttrBuilder, AttrType},
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix, MatrixData, MatrixDataType, iter::JitEntryType},
        wrapper::{Core, MatrixKind},
    },
    max_sys::{self, e_max_errorcodes::*, t_atom, t_jit_error_code as err, t_max_err},
    prelude::*,
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(JitMatrixObj)]
#[max(name = "jit.median.fluoride")]
pub struct Fluoride {
    glow: [Float64; 3],
    /// How many `glow` elements were last set, which is how many the getter returns.
    glowcount: AtomicUsize,
    #[attr(label = "Luminosity", category = "Behavior")]
    lum: Float64,
    #[attr(label = "Tolerance", category = "Behavior")]
    tol: Float64,
    #[attr(
        label = "Color Mode",
        category = "Behavior",
        enum = "\"Black and White\" Color"
    )]
    mode: Int64,
}

impl MatrixOp for Fluoride {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            glow: [0.0, 1.0, 0.0].map(Float64::new),
            glowcount: AtomicUsize::new(3),
            lum: 0.88.into(),
            tol: 0.2.into(),
            mode: 0i64.into(),
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
        // GAP: CLASS_ATTR_SYM_VARSIZE (array attributes)
        let mut b = AttrBuilder::new_accessors("glow", AttrType::Float64, glow_get, glow_set);
        b.style(c"rgb").label(c"Glow Color").category(c"Behavior");
        let attr = b.build().map_err(|_| median::error::MaxError::Generic)?;
        class.add_attribute(attr)
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let input = inputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        if common_kind(input, output)? != MatrixDataType::Char {
            return Err(err::JIT_ERR_MISMATCH_TYPE);
        }
        let input = Cells::<u8>::read(input)?;
        let glow = self.glow.each_ref().map(|g| (g.get() * 255.0) as i64);
        let lum = (self.lum.get() * 255.0) as i64;
        let tol = (self.tol.get() * 255.0) as i64;
        let bw = (lum - tol).max(1);
        let tmax = (lum + tol).min(255);
        // The C clips the attribute itself here (`CLIP_ASSIGN`).
        let mode = self.mode.get().clamp(0, 1);
        self.mode.set(mode);
        Cells::update(output, |out| {
            if input.planes != 4 || out.planes != 4 {
                return Err(err::JIT_ERR_MISMATCH_PLANE);
            }
            for cell in 0..out.cell_count() {
                let Some(source) = input.offset(&out.coord(cell), false) else {
                    continue;
                };
                let o = &mut out.data[cell * 4..cell * 4 + 4];
                let p = &input.data[source..source + 4];
                let index = ((p[1] as f64 * 0.299) as f32
                    + (p[2] as f64 * 0.587) as f32
                    + (p[3] as f64 * 0.114) as f32) as i64;
                o[0] = p[0];
                if index < bw {
                    if mode == 1 {
                        o.copy_from_slice(p);
                    } else {
                        o[1..].fill(index as u8);
                    }
                } else if index < lum + 1 {
                    let indperc = ((lum - index) as f32 as f64 * (1.0 / tol as f64)) as f32;
                    for k in 0..3 {
                        let value = if mode == 1 {
                            p[k + 1] as f32
                        } else {
                            index as f32
                        };
                        let outpix = (value * indperc) as i64;
                        let temp =
                            (glow[k] as f32 as f64 * (1.0 - indperc as f64) + outpix as f64) as i64;
                        o[k + 1] = temp.clamp(0, 255) as u8;
                    }
                } else {
                    let indperc =
                        ((255 - index) as f32 as f64 * (1.0 / (tmax - lum) as f64)) as f32;
                    for k in 0..3 {
                        o[k + 1] = ((glow[k] as f32 * indperc) as i64).clamp(0, 255) as u8;
                    }
                }
            }
            Ok(())
        })
    }
}

#[median::methods]
impl Fluoride {}

median::ext_main! { Fluoride }

type Wrapper = Core<Fluoride, MatrixKind>;

/// The `glow` getter: as many float atoms as were last set, in an array allocated as Jitter
/// expects if the one given is too small.
///
/// # Safety
/// Only Max calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn glow_get(
    w: &Wrapper,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "glow getter", MAX_ERR_GENERIC as _, || {
        if ac.is_null() || av.is_null() {
            return MAX_ERR_NONE as _;
        }
        let x = w.wrapped();
        let count = x.glowcount.load(Ordering::Relaxed);
        // SAFETY: `ac` and `av` are the getter's out-parameters; `*av` is null or holds `*ac`
        // atoms, and a new array is the caller's to free.
        unsafe {
            if (*av).is_null() || (*ac as usize) < count {
                // GAP: jit_getbytes
                let p = max_sys::jit_getbytes((count.max(1) * size_of::<t_atom>()) as c_long);
                if p.is_null() {
                    *ac = 0;
                    return MAX_ERR_OUT_OF_MEM as _;
                }
                *av = p.cast();
            }
            *ac = count as c_long;
            for (i, g) in x.glow.iter().take(count).enumerate() {
                (*av).add(i).cast::<Atom>().write(Atom::from(g.get()));
            }
        }
        MAX_ERR_NONE as _
    })
}

/// The `glow` setter: sets as many leading elements as it is given atoms, up to three.
///
/// # Safety
/// Only Max calls it, as an attribute setter, with `av` holding `ac` atoms.
unsafe extern "C" fn glow_set(
    w: &Wrapper,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "glow setter", MAX_ERR_GENERIC as _, || {
        let atoms: &[Atom] = if ac > 0 && !av.is_null() {
            // SAFETY: Max passes `ac` valid atoms; `Atom` is a transparent `t_atom`.
            unsafe { std::slice::from_raw_parts(av.cast(), ac as usize) }
        } else {
            &[]
        };
        let x = w.wrapped();
        let count = atoms.len().min(x.glow.len());
        for (field, atom) in x.glow.iter().zip(atoms) {
            field.set(atom.get_float());
        }
        x.glowcount.store(count, Ordering::Relaxed);
        MAX_ERR_NONE as _
    })
}

fn common_kind(input: &mut Matrix<'_>, output: &mut Matrix<'_>) -> JitResult<MatrixDataType> {
    let kind = input.lock().info()?.kind()?;
    if output.lock().info()?.kind()? != kind {
        return Err(err::JIT_ERR_MISMATCH_TYPE);
    }
    Ok(kind)
}

/// A copy of a matrix's cells, planes interleaved and dimension 0 fastest.
struct Cells<T> {
    dims: Vec<usize>,
    planes: usize,
    data: Vec<T>,
}

impl<T: JitEntryType> Cells<T> {
    fn read(matrix: &mut Matrix<'_>) -> JitResult<Self> {
        let mut lock = matrix.lock();
        Self::copy(&lock.data()?)
    }

    /// Copy the matrix, let `f` change the copy, and write it back.
    fn update(
        matrix: &mut Matrix<'_>,
        f: impl FnOnce(&mut Self) -> JitResult<()>,
    ) -> JitResult<()> {
        let mut lock = matrix.lock();
        let mut data = lock.data()?;
        let mut cells = Self::copy(&data)?;
        f(&mut cells)?;
        let mut source = cells.data.chunks_exact(cells.planes);
        // SAFETY: the output belongs to this matrix_calc call, and every input was copied (its
        // borrow ended) before, so an in-place output aliases no live reference.
        for mut chunk in unsafe { data.chunks_mut::<T>()? } {
            for cell in chunk.entry_iter_mut() {
                cell.copy_from_slice(source.next().ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?);
            }
        }
        Ok(())
    }

    fn copy(data: &MatrixData<'_>) -> JitResult<Self> {
        let info = data.info();
        let dims = info.dimensions()?.iter().map(|&d| d as usize).collect();
        let planes = info.planes()?;
        let mut cells = Vec::new();
        // SAFETY: matrix_calc's caller keeps the locked matrix initialized and unwritten while
        // it runs, and the borrow ends with this function.
        for chunk in unsafe { data.chunks::<T>()? } {
            for cell in chunk.entry_iter() {
                cells.extend_from_slice(cell);
            }
        }
        Ok(Self {
            dims,
            planes,
            data: cells,
        })
    }

    fn cell_count(&self) -> usize {
        self.data.len() / self.planes
    }

    fn coord(&self, mut cell: usize) -> [usize; 32] {
        let mut coord = [0; 32];
        for (c, &d) in coord.iter_mut().zip(&self.dims) {
            *c = cell % d;
            cell /= d;
        }
        coord
    }

    /// The index of the first plane of the cell at `coord`, or `None` outside the matrix. With
    /// `repeat`, a dimension of size 1 (or one the matrix lacks) matches every coordinate.
    fn offset(&self, coord: &[usize; 32], repeat: bool) -> Option<usize> {
        let (mut offset, mut stride) = (0, self.planes);
        for (i, &c) in coord.iter().enumerate() {
            let d = self.dims.get(i).copied().unwrap_or(1);
            let c = if repeat && d == 1 { 0 } else { c };
            if c >= d {
                return None;
            }
            offset += c * stride;
            stride *= d;
        }
        Some(offset)
    }
}

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use median::jit::matrix::{MatrixDataType, MatrixInfo};
    use median_mock::{
        Atom,
        jit::{MatrixFixture, MatrixList},
    };

    type Calc = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64;

    fn fixture(kind: MatrixDataType, planes: usize, dims: &[usize], bytes: &[u8]) -> MatrixFixture {
        let mut m = MatrixFixture::new(*MatrixInfo::new(kind, planes, dims).unwrap().as_raw());
        m.write_bytes(bytes);
        m
    }

    fn setup(k: &median_mock::Kernel, args: &[Atom]) -> (*mut c_void, *mut c_void, Calc) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let object = k.new_object("jit.median.fluoride", args);
        assert!(!object.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(object);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        // SAFETY: matrix_calc is registered as `t_jit_err f(x, inputs, outputs)`.
        (object, core, unsafe {
            std::mem::transmute::<usize, Calc>(f)
        })
    }

    fn run(calc: Calc, core: *mut c_void, input: &MatrixFixture, output: &MatrixFixture) -> i64 {
        let ins = MatrixList::new(&[input]);
        let outs = MatrixList::new(&[output]);
        // SAFETY: the core and lists are live for the call.
        unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
    }

    #[test]
    fn glow_is_a_three_element_array_attribute() {
        let k = median_mock::lock();
        let (object, _, _) = setup(&k, &[]);
        let floats = |v: &[f64]| v.iter().map(|&v| Atom::Float(v)).collect::<Vec<_>>();
        assert_eq!(
            k.attr_get(object, "glow").unwrap(),
            floats(&[0.0, 1.0, 0.0])
        );
        k.attr_set(object, "glow", &floats(&[0.5])).unwrap();
        assert_eq!(k.attr_get(object, "glow").unwrap(), floats(&[0.5]));
        k.attr_set(object, "glow", &floats(&[1.0, 0.0, 0.25, 9.0]))
            .unwrap();
        assert_eq!(
            k.attr_get(object, "glow").unwrap(),
            floats(&[1.0, 0.0, 0.25])
        );
        assert!(k.free_object(object));
    }

    #[test]
    fn glows_near_lum_and_passes_dark_cells() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(&k, &[]);
        // lum 224, tol 51: bw 173, tmax 255.
        let input = fixture(
            MatrixDataType::Char,
            4,
            &[3],
            &[9, 100, 50, 10, 9, 200, 200, 200, 9, 240, 240, 240],
        );
        let output = fixture(MatrixDataType::Char, 4, &[3], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        // index 60 passes as grey; 200 gives indperc 24/51 of itself and the rest glow green;
        // 240 fades the glow by 15/31.
        assert_eq!(
            output.bytes(),
            [9, 60, 60, 60, 9, 94, 228, 94, 9, 0, 123, 0]
        );

        k.attr_set(object, "mode", &[Atom::Long(5)]).unwrap();
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(&output.bytes()[..4], [9, 100, 50, 10]);
        assert_eq!(k.attr_get(object, "mode").unwrap(), [Atom::Long(1)]);
        assert!(k.free_object(object));
    }

    #[test]
    fn processes_the_smaller_domain_and_checks_planes() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(&k, &[]);
        let input = fixture(MatrixDataType::Char, 4, &[1, 1], &[1, 0, 0, 0]);
        let output = fixture(MatrixDataType::Char, 4, &[2, 1], &[7; 8]);
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), [1, 0, 0, 0, 7, 7, 7, 7]);

        let output = fixture(MatrixDataType::Char, 1, &[2, 1], &[]);
        assert_eq!(
            run(calc, core, &input, &output),
            median::max_sys::t_jit_error_code::JIT_ERR_MISMATCH_PLANE as i64
        );
        assert!(k.free_object(object));
    }
}
