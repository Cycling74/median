//! A port of the SDK's `matrix/jit.keyscreen`: a three-source chroma key on 4-plane char
//! matrices. Where the `key` input is within the tolerances of the key color, the output takes
//! the `mask` input, elsewhere the `target` input; per cell (`mode` 0) or per plane (`mode` 1).
//!
//! The C calculates with `jit_parallel_ndim_simplecalc4`; the port runs serially over median's
//! chunk views. As in the C, `key`, `target` and `mask` select the left input with 0, the right
//! one with 1 and the middle one with 2. Each input is read with its own layout; the C reads the
//! mask with the wrong row stride when it is not the middle input (it tests `mask==1` twice).
//! The attributes are not basic.

use median::{
    jit::{
        JitBuilder, JitObjRef, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix, MatrixData, MatrixDataType, iter::JitEntryType},
    },
    max_sys::t_jit_error_code as err,
    prelude::*,
};

#[derive(JitMatrixObj)]
#[max(name = "jit.median.keyscreen")]
pub struct Keyscreen {
    obj: JitObjRef,
    #[attr(label = "Keying Mode", category = "Behavior", enum = "Wrap Clip Fold")]
    mode: Int64,
    #[attr(label = "Key Input", category = "Behavior")]
    key: Int64,
    #[attr(label = "Target Input", category = "Behavior")]
    target: Int64,
    #[attr(label = "Mask Input", category = "Behavior")]
    mask: Int64,
    #[attr(label = "Alpha", category = "Behavior")]
    alpha: Float32,
    #[attr(label = "Red", category = "Behavior")]
    red: Float32,
    #[attr(label = "Green", category = "Behavior")]
    green: Float32,
    #[attr(label = "Blue", category = "Behavior")]
    blue: Float32,
    #[attr(label = "Alpha Tolerance", category = "Behavior")]
    alphatol: Float32,
    #[attr(label = "Red Tolerance", category = "Behavior")]
    redtol: Float32,
    #[attr(label = "Green Tolerance", category = "Behavior")]
    greentol: Float32,
    #[attr(label = "Blue Tolerance", category = "Behavior")]
    bluetol: Float32,
}

impl MatrixOp for Keyscreen {
    fn new(builder: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            obj: builder.obj(),
            mode: 0i64.into(),
            key: 0i64.into(),
            target: 1i64.into(),
            mask: 2i64.into(),
            alpha: 0.0.into(),
            red: 0.0.into(),
            green: 0.0.into(),
            blue: 0.0.into(),
            alphatol: 0.0.into(),
            redtol: 0.0.into(),
            greentol: 0.0.into(),
            bluetol: 0.0.into(),
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(3),
            outputs: Count::Fixed(1),
        }
    }

    fn mop_setup(mop: &mut MOP<'_>) -> JitResult<()> {
        mop.single_type(MatrixDataType::Char)?;
        mop.single_plane_count(4)
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let [first, second, third] = inputs else {
            return Err(err::JIT_ERR_INVALID_PTR);
        };
        let out_kind = output.lock().info()?.kind()?;
        if first.lock().info()?.kind()? != MatrixDataType::Char || out_kind != MatrixDataType::Char
        {
            return Err(err::JIT_ERR_MISMATCH_TYPE);
        }
        let first = Cells::<u8>::read(first)?;
        if first.planes != 4 {
            return Err(err::JIT_ERR_MISMATCH_PLANE);
        }
        let mut sources = vec![first];
        for input in [second, third] {
            if input.lock().info()?.kind()? != MatrixDataType::Char {
                self.obj.post("type not ok");
                return Err(err::JIT_ERR_MISMATCH_TYPE);
            }
            let cells = Cells::<u8>::read(input)?;
            if cells.planes != 4 {
                self.obj.post("planes not ok");
                return Err(err::JIT_ERR_MISMATCH_PLANE);
            }
            sources.push(cells);
        }
        let scaled = |v: &Float32| (v.get() as f64 * 255.0) as i64;
        let color = [&self.alpha, &self.red, &self.green, &self.blue].map(scaled);
        let tolerance = [&self.alphatol, &self.redtol, &self.greentol, &self.bluetol].map(scaled);
        let low: [i64; 4] = std::array::from_fn(|i| color[i] - tolerance[i]);
        let high: [i64; 4] = std::array::from_fn(|i| color[i] + tolerance[i]);
        let key = &sources[match self.key.get() {
            1 => 2,
            2 => 1,
            _ => 0,
        }];
        let target = &sources[match self.target.get() {
            0 => 0,
            2 => 1,
            _ => 2,
        }];
        let mask = &sources[match self.mask.get() {
            0 => 0,
            1 => 2,
            _ => 1,
        }];
        let per_plane = self.mode.get() == 1;
        Cells::update(output, |out| {
            if out.planes != 4 {
                return Err(err::JIT_ERR_MISMATCH_PLANE);
            }
            for cell in 0..out.cell_count() {
                let coord = out.coord(cell);
                let (Some(k), Some(t), Some(m)) = (
                    key.offset(&coord, false),
                    target.offset(&coord, false),
                    mask.offset(&coord, false),
                ) else {
                    continue;
                };
                let keyed = |p: usize| (low[p]..=high[p]).contains(&(key.data[k + p] as i64));
                let all_keyed = (0..4).all(keyed);
                for p in 0..4 {
                    let use_mask = if per_plane { keyed(p) } else { all_keyed };
                    out.data[cell * 4 + p] = if use_mask {
                        mask.data[m + p]
                    } else {
                        target.data[t + p]
                    };
                }
            }
            Ok(())
        })
    }
}

#[median::methods]
impl Keyscreen {}

median::ext_main! { Keyscreen }

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

    fn fixture(planes: usize, dims: &[usize], bytes: &[u8]) -> MatrixFixture {
        let info = MatrixInfo::new(MatrixDataType::Char, planes, dims).unwrap();
        let mut m = MatrixFixture::new(*info.as_raw());
        m.write_bytes(bytes);
        m
    }

    fn setup(k: &median_mock::Kernel, args: &[Atom]) -> (*mut c_void, *mut c_void, Calc) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let object = k.new_object("jit.median.keyscreen", args);
        assert!(!object.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(object);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        // SAFETY: matrix_calc is registered as `t_jit_err f(x, inputs, outputs)`.
        (object, core, unsafe {
            std::mem::transmute::<usize, Calc>(f)
        })
    }

    fn run(
        calc: Calc,
        core: *mut c_void,
        inputs: [&MatrixFixture; 3],
        output: &MatrixFixture,
    ) -> i64 {
        let ins = MatrixList::new(&inputs);
        let outs = MatrixList::new(&[output]);
        // SAFETY: the core and lists are live for the call.
        unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
    }

    #[test]
    fn keys_per_cell_and_per_plane() {
        let k = median_mock::lock();
        // Key color 255 0 255 0 with a tolerance of 0.2 (51) on every plane.
        let mut args: Vec<Atom> = vec!["@alpha".into(), 1.0.into(), "@green".into(), 1.0.into()];
        for tol in ["@alphatol", "@redtol", "@greentol", "@bluetol"] {
            args.extend([tol.into(), 0.2.into()]);
        }
        let (object, core, calc) = setup(&k, &args);
        assert_eq!(k.attr_get(object, "target").unwrap(), [Atom::Long(1)]);
        let key = fixture(4, &[2], &[250, 40, 210, 10, 250, 100, 210, 10]);
        let mask = fixture(4, &[2], &[1, 2, 3, 4, 5, 6, 7, 8]);
        let target = fixture(4, &[2], &[11, 12, 13, 14, 15, 16, 17, 18]);
        let output = fixture(4, &[2], &[]);
        assert_eq!(run(calc, core, [&key, &mask, &target], &output), 0);
        assert_eq!(output.bytes(), [1, 2, 3, 4, 15, 16, 17, 18]);

        k.attr_set(object, "mode", &[Atom::Long(1)]).unwrap();
        assert_eq!(run(calc, core, [&key, &mask, &target], &output), 0);
        assert_eq!(output.bytes(), [1, 2, 3, 4, 5, 16, 7, 8]);
        assert!(k.free_object(object));
    }

    #[test]
    fn selects_inputs_with_the_c_numbering() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(&k, &[]);
        let left = fixture(4, &[1], &[0, 0, 0, 0]);
        let middle = fixture(4, &[1], &[2, 2, 2, 2]);
        let right = fixture(4, &[1], &[1, 1, 1, 1]);
        let output = fixture(4, &[1], &[]);
        // The left key matches the default black key color, so the mask (2: middle) is out.
        assert_eq!(run(calc, core, [&left, &middle, &right], &output), 0);
        assert_eq!(output.bytes(), [2; 4]);
        // Key 2 (the middle) does not match: the target (1: right) is out.
        k.attr_set(object, "key", &[Atom::Long(2)]).unwrap();
        assert_eq!(run(calc, core, [&left, &middle, &right], &output), 0);
        assert_eq!(output.bytes(), [1; 4]);
        assert!(k.free_object(object));
    }

    #[test]
    fn processes_the_common_domain_and_checks_the_inputs() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(&k, &[]);
        let small = fixture(4, &[1], &[0; 4]);
        let big = fixture(4, &[2], &[3; 8]);
        let output = fixture(4, &[2], &[9; 8]);
        assert_eq!(run(calc, core, [&big, &small, &big], &output), 0);
        assert_eq!(output.bytes(), [3, 3, 3, 3, 9, 9, 9, 9]);

        k.take_console_log();
        let one_plane = fixture(1, &[2], &[0; 2]);
        assert_eq!(
            run(calc, core, [&big, &one_plane, &big], &output),
            median::max_sys::t_jit_error_code::JIT_ERR_MISMATCH_PLANE as i64
        );
        let log = k.take_console_log();
        assert!(log.iter().any(|e| e.text == "planes not ok"), "{log:?}");
        assert!(k.free_object(object));
    }
}
