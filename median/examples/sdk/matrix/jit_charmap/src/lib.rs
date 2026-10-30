//! A port of the SDK's `matrix/jit.charmap`: maps each char plane of the left input through a
//! 256-cell lookup table, the right input.
//!
//! The C calculates in parallel (`jit_parallel_ndim_simplecalc3` with
//! `JIT_PARALLEL_NDIM_FLAGS_FULL_MATRIX` on the table); this port calculates serially. The right
//! input's `dimlink 0` and its 256-cell `mindim`/`maxdim` are set on the MOP's io with raw calls;
//! median-mock answers `getinput` with null, so that is untested.
//! Deviations, both fixing the C: a table narrower than 256 cells is an error with the output
//! left as it was (the C reports it but still reads past the table), and a left input dimension
//! of size 1 is repeated across the output in every dimension (the C does so only in the first).

use core::ffi::c_long;
use median::{
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix, MatrixDataType},
    },
    max_sys::{self, t_jit_error_code as err},
    prelude::*,
};

const TABLE_SIZE: usize = 256;

#[derive(JitMatrixObj)]
#[max(name = "jit.median.charmap", no_methods)]
pub struct CharMap;

impl MatrixOp for CharMap {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self)
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(2),
            outputs: Count::Fixed(1),
        }
    }

    fn mop_setup(mop: &mut MOP<'_>) -> JitResult<()> {
        mop.single_type(MatrixDataType::Char)?;
        let off = Atom::from(0i64);
        let size = Atom::from(TABLE_SIZE as i64);
        // SAFETY: `mop` is a live MOP being configured during registration; the atoms outlive
        // the calls, which copy them.
        // GAP: per-io MOP configuration
        unsafe {
            let table =
                max_sys::jit_object_method(mop.as_ptr(), sym!("getinput").as_ptr(), 2 as c_long);
            if !table.is_null() {
                for (selector, value) in [
                    (sym!("dimlink"), &off),
                    (sym!("mindim"), &size),
                    (sym!("maxdim"), &size),
                ] {
                    max_sys::jit_object_method(
                        table,
                        selector.as_ptr(),
                        1 as c_long,
                        std::ptr::from_ref(value),
                    );
                }
            }
        }
        Ok(())
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let [input, table] = inputs else {
            return Err(err::JIT_ERR_INVALID_PTR);
        };
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let input = Snapshot::read(input)?;
        let table = Snapshot::read(table)?;
        let mut lock = output.lock();
        let mut data = lock.data().map_err(|_| err::JIT_ERR_INVALID_OUTPUT)?;
        let info = *data.info();
        if input.kind != table.kind || input.kind != info.kind()? {
            return Err(err::JIT_ERR_MISMATCH_TYPE);
        }
        if table.dim(0) < TABLE_SIZE {
            return Err(err::JIT_ERR_MISMATCH_DIM);
        }
        if input.kind != MatrixDataType::Char {
            return Ok(());
        }
        let out_dims: Vec<usize> = info.dimensions()?.iter().map(|&d| d as usize).collect();
        let limits: Vec<usize> = out_dims
            .iter()
            .enumerate()
            .map(|(i, &dim)| match input.dim(i) {
                1 => dim,
                d => dim.min(d),
            })
            .collect();
        let mut coord = vec![0; out_dims.len()];
        let mut linear = 0;
        // SAFETY: the output belongs to this matrix_calc call; the inputs were copied, so an
        // in-place output aliases no live borrow.
        for mut chunk in unsafe { data.chunks_mut::<u8>()? } {
            for cell in chunk.entry_iter_mut() {
                decompose(linear, &out_dims, &mut coord);
                linear += 1;
                if coord.iter().zip(&limits).any(|(c, l)| c >= l) {
                    continue;
                }
                let source = input.cell(&coord);
                for (j, out) in cell.iter_mut().enumerate() {
                    let index = source[j % source.len()] as usize;
                    *out = table.cell(&[index])[j % table.planes];
                }
            }
        }
        Ok(())
    }
}

fn decompose(mut linear: usize, dims: &[usize], coord: &mut [usize]) {
    for (c, &dim) in coord.iter_mut().zip(dims) {
        *c = linear % dim;
        linear /= dim;
    }
}

/// A dense copy of a char matrix, dimension 0 fastest; empty for other types.
struct Snapshot {
    kind: MatrixDataType,
    dims: Vec<usize>,
    planes: usize,
    cells: Vec<u8>,
}

impl Snapshot {
    fn read(matrix: &mut Matrix<'_>) -> JitResult<Self> {
        let mut lock = matrix.lock();
        let data = lock.data().map_err(|_| err::JIT_ERR_INVALID_INPUT)?;
        let info = data.info();
        let kind = info.kind()?;
        let mut cells = Vec::new();
        if kind == MatrixDataType::Char {
            // SAFETY: inputs are stable and unwritten while matrix_calc runs; the borrow ends
            // here.
            for chunk in unsafe { data.chunks::<u8>()? } {
                for cell in chunk.entry_iter() {
                    cells.extend_from_slice(cell);
                }
            }
        }
        Ok(Self {
            kind,
            dims: info.dimensions()?.iter().map(|&d| d as usize).collect(),
            planes: info.planes()?,
            cells,
        })
    }

    /// The size of dimension `i`; 1 past the matrix's dimensions.
    fn dim(&self, i: usize) -> usize {
        self.dims.get(i).copied().unwrap_or(1)
    }

    /// The cell at `coord`, repeating dimensions of size 1; missing coordinates are 0.
    fn cell(&self, coord: &[usize]) -> &[u8] {
        let (mut index, mut stride) = (0, 1);
        for (&c, &dim) in coord.iter().zip(&self.dims) {
            index += if dim == 1 { 0 } else { c } * stride;
            stride *= dim;
        }
        &self.cells[index * self.planes..][..self.planes]
    }
}

median::ext_main! { CharMap }

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use median::jit::matrix::{MatrixDataType, MatrixInfo};
    use median_mock::jit::{MatrixFixture, MatrixList};

    type Calc = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64;

    fn fixture(kind: MatrixDataType, planes: usize, dims: &[usize], bytes: &[u8]) -> MatrixFixture {
        let mut m = MatrixFixture::new(*MatrixInfo::new(kind, planes, dims).unwrap().as_raw());
        m.write_bytes(bytes);
        m
    }

    fn setup(k: &median_mock::Kernel) -> (*mut c_void, *mut c_void, Calc) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.charmap", &[]);
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

    /// A 4-plane table whose plane `p` maps `v` to `v + p` (wrapping) and plane 0 inverts.
    fn table() -> MatrixFixture {
        let bytes: Vec<u8> = (0..=255u8)
            .flat_map(|v| {
                [
                    255 - v,
                    v.wrapping_add(1),
                    v.wrapping_add(2),
                    v.wrapping_add(3),
                ]
            })
            .collect();
        fixture(MatrixDataType::Char, 4, &[256], &bytes)
    }

    #[test]
    fn maps_each_plane_through_its_table_plane() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k);
        let input = fixture(
            MatrixDataType::Char,
            4,
            &[2],
            &[0, 10, 20, 255, 200, 1, 2, 3],
        );
        let out = fixture(MatrixDataType::Char, 4, &[2], &[0; 8]);
        let table = table();
        assert_eq!(run(calc, core, &[&input, &table], &out), 0);
        assert_eq!(out.bytes(), [255, 11, 22, 2, 55, 2, 4, 6]);
        for m in [&input, &table, &out] {
            assert_eq!(m.lock_state(), 0);
        }
        assert_eq!(k.outlet_count(x), 2);
        assert_eq!(k.inlets(x).len(), 1, "the right matrix inlet");
        assert!(k.free_object(x));
    }

    #[test]
    fn wraps_planes_and_repeats_a_unit_input() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k);
        // One plane, one cell: every output cell and plane looks it up.
        let input = fixture(MatrixDataType::Char, 1, &[1, 1], &[5]);
        let out = fixture(MatrixDataType::Char, 2, &[2, 2], &[0; 8]);
        let table = table();
        assert_eq!(run(calc, core, &[&input, &table], &out), 0);
        assert_eq!(out.bytes(), [250, 6, 250, 6, 250, 6, 250, 6]);

        // A narrower input truncates the output.
        let narrow = fixture(MatrixDataType::Char, 2, &[2, 1, 1], &[0, 0, 1, 1]);
        let out = fixture(MatrixDataType::Char, 2, &[3, 1, 2], &[9; 12]);
        assert_eq!(run(calc, core, &[&narrow, &table], &out), 0);
        assert_eq!(out.bytes(), [255, 1, 254, 2, 9, 9, 255, 1, 254, 2, 9, 9]);
        assert!(k.free_object(x));
    }

    #[test]
    fn refuses_a_short_table_and_mixed_types() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k);
        let input = fixture(MatrixDataType::Char, 1, &[1], &[7]);
        let out = fixture(MatrixDataType::Char, 1, &[1], &[9]);
        let short = fixture(MatrixDataType::Char, 1, &[255], &[1; 255]);
        assert_ne!(run(calc, core, &[&input, &short], &out), 0);
        assert_eq!(out.bytes(), [9]);
        let float = fixture(MatrixDataType::Float32, 1, &[256], &[0; 1024]);
        assert_ne!(run(calc, core, &[&input, &float], &out), 0);
        assert!(k.free_object(x));
    }
}
