//! A port of the SDK's `matrix/jit.alphablend`: crossfades two matrices cell by cell, weighted by
//! the first plane (alpha) of the left input.
//!
//! The C calculates in parallel (`jit_parallel_ndim_simplecalc3`); this port calculates serially.
//! The right input and the output accept only char, float32 and float64, set on the MOP's ios with
//! raw calls; median-mock answers `getinput`/`getoutput` with null, so that is untested.
//! Deviations: a right input dimension of size 1 is repeated across the output (the C skips it
//! when truncating the output but then reads past it), and the float64 alpha is kept in double
//! precision (the C narrows it to float).

use core::ffi::c_long;
use median::{
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix, MatrixDataType, iter::JitEntryType},
    },
    max_sys::{self, t_jit_error_code as err},
    prelude::*,
    symbol::SymbolRef,
};

#[derive(JitMatrixObj)]
#[max(name = "jit.median.alphablend", no_methods)]
pub struct AlphaBlend {
    #[attr(
        label = "Crossfade Mode",
        enum = "Left-to-Right Right-to-Left",
        category = "Mode"
    )]
    mode: Int64,
}

impl MatrixOp for AlphaBlend {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { mode: 0i64.into() })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(2),
            outputs: Count::Fixed(1),
        }
    }

    fn mop_setup(mop: &mut MOP<'_>) -> JitResult<()> {
        let types: [Atom; 3] = [sym!("char"), sym!("float32"), sym!("float64")].map(Atom::from);
        // SAFETY: `mop` is a live MOP being configured during registration; `types` outlives the
        // calls, which copy it.
        // GAP: per-io MOP configuration
        unsafe {
            io_message(mop, sym!("getinput"), 2, sym!("types"), &types);
            io_message(mop, sym!("getoutput"), 1, sym!("types"), &types);
        }
        Ok(())
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let [left, right] = inputs else {
            return Err(err::JIT_ERR_INVALID_PTR);
        };
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let kind = left.lock().info()?.kind()?;
        let inverse = self.mode.get() == 1;
        match kind {
            MatrixDataType::Char => blend::<u8>(left, right, output, inverse),
            MatrixDataType::Float32 => blend::<f32>(left, right, output, inverse),
            MatrixDataType::Float64 => blend::<f64>(left, right, output, inverse),
            MatrixDataType::Int32 => Err(err::JIT_ERR_MISMATCH_TYPE),
        }
    }
}

/// Sends `selector` with `atoms` to the MOP io that `getter` (`getinput`/`getoutput`) returns for
/// the 1-based `index`. Does nothing if there is no such io.
///
/// # Safety
/// `mop` must be configurable (class registration).
unsafe fn io_message(
    mop: &MOP<'_>,
    getter: SymbolRef,
    index: c_long,
    selector: SymbolRef,
    atoms: &[Atom],
) {
    unsafe {
        let io = max_sys::jit_object_method(mop.as_ptr(), getter.as_ptr(), index);
        if !io.is_null() {
            max_sys::jit_object_method(
                io,
                selector.as_ptr(),
                atoms.len() as c_long,
                atoms.as_ptr(),
            );
        }
    }
}

trait Blend: JitEntryType {
    /// Writes an opaque alpha and the other planes of `a` and `b` mixed by `a`'s alpha.
    fn blend(out: &mut [Self], a: &[Self], b: &[Self], inverse: bool);
}

impl Blend for u8 {
    fn blend(out: &mut [u8], a: &[u8], b: &[u8], inverse: bool) {
        let alpha = if inverse {
            256 - a[0] as i32
        } else {
            a[0] as i32
        };
        out[0] = 255;
        for k in 1..out.len() {
            out[k] = ((a[k] as i32 * alpha + b[k] as i32 * (256 - alpha)) >> 8) as u8;
        }
    }
}

macro_rules! float_blend {
    ($($t:ty),*) => { $(
        impl Blend for $t {
            fn blend(out: &mut [$t], a: &[$t], b: &[$t], inverse: bool) {
                let clipped = a[0].clamp(0.0, 1.0);
                let alpha = if inverse { 1.0 - clipped } else { clipped };
                out[0] = 1.0;
                for k in 1..out.len() {
                    out[k] = a[k] * alpha + b[k] * (1.0 - alpha);
                }
            }
        }
    )* };
}
float_blend!(f32, f64);

fn blend<T: Blend>(
    left: &mut Matrix<'_>,
    right: &mut Matrix<'_>,
    output: &mut Matrix<'_>,
    inverse: bool,
) -> JitResult<()> {
    let left = Snapshot::<T>::read(left)?;
    let right = Snapshot::<T>::read(right)?;
    if left.planes != right.planes {
        return Err(err::JIT_ERR_MISMATCH_PLANE);
    }
    let mut lock = output.lock();
    let mut data = lock.data().map_err(|_| err::JIT_ERR_INVALID_OUTPUT)?;
    let info = *data.info();
    if info.kind()? != T::KIND {
        return Err(err::JIT_ERR_MISMATCH_TYPE);
    }
    if info.planes()? != left.planes {
        return Err(err::JIT_ERR_MISMATCH_PLANE);
    }
    let out_dims: Vec<usize> = info.dimensions()?.iter().map(|&d| d as usize).collect();
    let limits: Vec<usize> = out_dims
        .iter()
        .enumerate()
        .map(|(i, &dim)| {
            let (l, r) = (left.dim(i), right.dim(i));
            let dim = dim.min(l);
            if r > 1 { dim.min(r) } else { dim }
        })
        .collect();
    let mut coord = vec![0; out_dims.len()];
    let mut linear = 0;
    // SAFETY: the output belongs to this matrix_calc call; the inputs were copied, so an in-place
    // output aliases no live borrow.
    for mut chunk in unsafe { data.chunks_mut::<T>()? } {
        for cell in chunk.entry_iter_mut() {
            decompose(linear, &out_dims, &mut coord);
            linear += 1;
            if coord.iter().zip(&limits).all(|(c, l)| c < l) {
                T::blend(cell, left.cell(&coord), right.cell(&coord), inverse);
            }
        }
    }
    Ok(())
}

fn decompose(mut linear: usize, dims: &[usize], coord: &mut [usize]) {
    for (c, &dim) in coord.iter_mut().zip(dims) {
        *c = linear % dim;
        linear /= dim;
    }
}

/// A dense copy of a matrix, dimension 0 fastest.
struct Snapshot<T> {
    dims: Vec<usize>,
    planes: usize,
    cells: Vec<T>,
}

impl<T: JitEntryType> Snapshot<T> {
    fn read(matrix: &mut Matrix<'_>) -> JitResult<Self> {
        let mut lock = matrix.lock();
        let data = lock.data().map_err(|_| err::JIT_ERR_INVALID_INPUT)?;
        let info = data.info();
        let mut cells = Vec::new();
        // SAFETY: inputs are stable and unwritten while matrix_calc runs; the borrow ends here.
        for chunk in unsafe { data.chunks::<T>()? } {
            for cell in chunk.entry_iter() {
                cells.extend_from_slice(cell);
            }
        }
        Ok(Self {
            dims: info.dimensions()?.iter().map(|&d| d as usize).collect(),
            planes: info.planes()?,
            cells,
        })
    }

    /// The size of dimension `i`; 1 past the matrix's dimensions.
    fn dim(&self, i: usize) -> usize {
        self.dims.get(i).copied().unwrap_or(1)
    }

    /// The cell at `coord`, repeating dimensions of size 1.
    fn cell(&self, coord: &[usize]) -> &[T] {
        let (mut index, mut stride) = (0, 1);
        for (&c, &dim) in coord.iter().zip(&self.dims) {
            index += if dim == 1 { 0 } else { c } * stride;
            stride *= dim;
        }
        &self.cells[index * self.planes..][..self.planes]
    }
}

median::ext_main! { AlphaBlend }

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

    fn floats(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    fn setup(k: &median_mock::Kernel) -> (*mut c_void, *mut c_void, Calc) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.alphablend", &[]);
        assert!(!x.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        (x, core, unsafe { std::mem::transmute::<usize, Calc>(f) })
    }

    fn run(calc: Calc, core: *mut c_void, ins: &[&MatrixFixture], out: &MatrixFixture) -> i64 {
        let ins = MatrixList::new(ins);
        let outs = MatrixList::new(&[out]);
        unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
    }

    #[test]
    fn blends_char_by_left_alpha_in_both_modes() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k);
        let char4 = |bytes: &[u8]| fixture(MatrixDataType::Char, 4, &[2, 1], bytes);
        let a = char4(&[0, 200, 100, 0, 128, 200, 100, 0]);
        let b = char4(&[9, 0, 100, 255, 9, 0, 100, 255]);
        let out = char4(&[0; 8]);
        assert_eq!(run(calc, core, &[&a, &b], &out), 0);
        assert_eq!(out.bytes(), [255, 0, 100, 255, 255, 100, 100, 127]);
        k.attr_set(x, "mode", &[Atom::Long(1)]).unwrap();
        assert_eq!(run(calc, core, &[&a, &b], &out), 0);
        assert_eq!(out.bytes(), [255, 200, 100, 0, 255, 100, 100, 127]);
        for m in [&a, &b, &out] {
            assert_eq!(m.lock_state(), 0);
        }
        assert!(k.free_object(x));
    }

    #[test]
    fn blends_float_with_clipped_alpha_and_repeats_unit_dimensions() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k);
        let a = fixture(
            MatrixDataType::Float32,
            2,
            &[2, 2],
            &floats(&[0.25, 1.0, 2.0, 1.0, -1.0, 1.0, 0.5, 1.0]),
        );
        // One row, repeated down the output.
        let b = fixture(
            MatrixDataType::Float32,
            2,
            &[2, 1],
            &floats(&[0.0, 3.0, 0.0, 3.0]),
        );
        let out = fixture(MatrixDataType::Float32, 2, &[2, 2], &[0; 16]);
        assert_eq!(run(calc, core, &[&a, &b], &out), 0);
        assert_eq!(
            out.bytes(),
            floats(&[1.0, 2.5, 1.0, 1.0, 1.0, 3.0, 1.0, 2.0])
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn truncates_to_the_inputs_and_refuses_mismatches() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k);
        let a = fixture(MatrixDataType::Char, 1, &[2], &[255, 255]);
        let short = fixture(MatrixDataType::Char, 1, &[1], &[0]);
        let out = fixture(MatrixDataType::Char, 1, &[3], &[7, 7, 7]);
        assert_eq!(run(calc, core, &[&short, &a], &out), 0);
        assert_eq!(out.bytes(), [255, 7, 7]);

        let two = fixture(MatrixDataType::Char, 2, &[2], &[0; 4]);
        assert_ne!(run(calc, core, &[&a, &two], &out), 0);
        let float = fixture(MatrixDataType::Float32, 1, &[2], &[0; 8]);
        assert_ne!(run(calc, core, &[&a, &float], &out), 0);
        let long = fixture(MatrixDataType::Int32, 1, &[2], &[0; 8]);
        assert_ne!(run(calc, core, &[&long, &long], &out), 0);
        assert!(k.free_object(x));
    }

    #[test]
    fn has_two_matrix_inlets_and_a_mode_menu() {
        let k = median_mock::lock();
        let (x, core, _) = setup(&k);
        assert_eq!(k.outlet_count(x), 2);
        assert_eq!(k.inlets(x).len(), 1, "the right matrix inlet");
        let (_, style) = k.attr_attr(core, "mode", "style").unwrap();
        assert_eq!(style, [Atom::Sym("enumindex".into())]);
        let (_, label) = k.attr_attr(core, "mode", "label").unwrap();
        assert_eq!(label, [Atom::Sym("Crossfade Mode".into())]);
        assert!(k.free_object(x));
    }
}
