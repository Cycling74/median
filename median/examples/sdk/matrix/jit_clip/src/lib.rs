//! A port of the SDK's `matrix/jit.clip`: clamps every cell of a matrix of any type to its `min`
//! and `max` attributes (scaled by 255 for char matrices).
//!
//! The C calculates with `jit_parallel_ndim_simplecalc2`; the port runs serially over median's
//! chunk views, with the same domain: an input dimension of size 1 repeats across the output, a
//! smaller one leaves the rest of the output untouched, and output plane `p` reads input plane
//! `p % planes`. The attributes are not basic (`CLASS_STICKY_ATTR "basic"` in the C).

use median::{
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, Matrix, MatrixData, MatrixDataType, iter::JitEntryType},
    },
    max_sys::t_jit_error_code as err,
    prelude::*,
};

#[derive(JitMatrixObj)]
#[max(name = "jit.median.clip")]
pub struct Clip {
    #[attr(label = "Minimum", category = "Behavior")]
    min: Float64,
    #[attr(label = "Maximum", category = "Behavior")]
    max: Float64,
}

impl MatrixOp for Clip {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            min: 0.0.into(),
            max: 1.0.into(),
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let input = inputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let (min, max) = (self.min.get(), self.max.get());
        match common_kind(input, output)? {
            MatrixDataType::Char => {
                let (lo, hi) = (char_limit(min), char_limit(max));
                map_cells(input, output, |v: u8| clip(v as i64, lo, hi) as u8)
            }
            MatrixDataType::Int32 => {
                let (lo, hi) = (min as i64, max as i64);
                map_cells(input, output, |v: i32| clip(v as i64, lo, hi) as i32)
            }
            MatrixDataType::Float32 => {
                let (lo, hi) = (min as f32, max as f32);
                map_cells(input, output, |v: f32| clip(v, lo, hi))
            }
            MatrixDataType::Float64 => map_cells(input, output, |v: f64| clip(v, min, max)),
        }
    }
}

#[median::methods]
impl Clip {}

median::ext_main! { Clip }

fn char_limit(v: f64) -> i64 {
    ((v * 255.0) as i64).clamp(0, 255)
}

/// The C's `tmp>max?max:tmp<min?min:tmp`, which, unlike `clamp`, accepts `min > max`.
fn clip<T: PartialOrd>(v: T, min: T, max: T) -> T {
    if v > max {
        max
    } else if v < min {
        min
    } else {
        v
    }
}

fn common_kind(input: &mut Matrix<'_>, output: &mut Matrix<'_>) -> JitResult<MatrixDataType> {
    let kind = input.lock().info()?.kind()?;
    if output.lock().info()?.kind()? != kind {
        return Err(err::JIT_ERR_MISMATCH_TYPE);
    }
    Ok(kind)
}

/// Write `f` of each input cell to the output cells it covers.
fn map_cells<T: JitEntryType>(
    input: &mut Matrix<'_>,
    output: &mut Matrix<'_>,
    f: impl Fn(T) -> T,
) -> JitResult<()> {
    let input = Cells::<T>::read(input)?;
    Cells::update(output, |out| {
        for cell in 0..out.cell_count() {
            let Some(source) = input.offset(&out.coord(cell), true) else {
                continue;
            };
            for plane in 0..out.planes {
                out.data[cell * out.planes + plane] = f(input.data[source + plane % input.planes]);
            }
        }
        Ok(())
    })
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
        let object = k.new_object("jit.median.clip", args);
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

    fn f64_bytes(values: &[f64]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    #[test]
    fn clamps_char_scaled_by_255_and_floats_unscaled() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(&k, &["@min".into(), 0.2.into()]);
        assert_eq!(k.attr_get(object, "max").unwrap(), [Atom::Float(1.0)]);
        assert_eq!(
            k.attr_attr(core, "min", "label").map(|(_, a)| a),
            Some(vec![Atom::Sym("Minimum".into())])
        );
        k.attr_set(object, "max", &[Atom::Float(0.5)]).unwrap();
        let input = fixture(MatrixDataType::Char, 1, &[4], &[0, 60, 100, 255]);
        let output = fixture(MatrixDataType::Char, 1, &[4], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), [51, 60, 100, 127]);

        let input = fixture(
            MatrixDataType::Float64,
            2,
            &[2],
            &f64_bytes(&[-1.0, 0.3, 0.7, 9.0]),
        );
        let output = fixture(MatrixDataType::Float64, 2, &[2], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), f64_bytes(&[0.2, 0.3, 0.5, 0.5]));
        assert!(k.free_object(object));
    }

    #[test]
    fn long_limits_truncate_and_inverted_limits_follow_the_c() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(
            &k,
            &["@min".into(), (-2.7).into(), "@max".into(), 3.9.into()],
        );
        let longs: Vec<u8> = [-5i32, 0, 7].iter().flat_map(|v| v.to_ne_bytes()).collect();
        let input = fixture(MatrixDataType::Int32, 1, &[3], &longs);
        let output = fixture(MatrixDataType::Int32, 1, &[3], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        let expected: Vec<u8> = [-2i32, 0, 3].iter().flat_map(|v| v.to_ne_bytes()).collect();
        assert_eq!(output.bytes(), expected);

        k.attr_set(object, "min", &[Atom::Float(0.8)]).unwrap();
        k.attr_set(object, "max", &[Atom::Float(0.2)]).unwrap();
        let input = fixture(
            MatrixDataType::Float64,
            1,
            &[3],
            &f64_bytes(&[0.1, 0.5, 0.9]),
        );
        let output = fixture(MatrixDataType::Float64, 1, &[3], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), f64_bytes(&[0.8, 0.2, 0.2]));
        assert!(k.free_object(object));
    }

    #[test]
    fn repeats_size_one_dimensions_and_refuses_mismatched_types() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(&k, &[]);
        let input = fixture(MatrixDataType::Float64, 1, &[1, 1], &f64_bytes(&[2.0]));
        let output = fixture(MatrixDataType::Float64, 1, &[2, 2], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), f64_bytes(&[1.0; 4]));

        let output = fixture(MatrixDataType::Char, 1, &[2, 2], &[]);
        assert_eq!(
            run(calc, core, &input, &output),
            median::max_sys::t_jit_error_code::JIT_ERR_MISMATCH_TYPE as i64
        );
        assert!(k.free_object(object));
    }
}
