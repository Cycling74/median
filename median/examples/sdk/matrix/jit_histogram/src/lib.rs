//! A port of the SDK's `matrix/jit.histogram`: counts the values of each plane of a char or long
//! matrix into a long output matrix, one cell per value.
//!
//! Deviations from the C:
//! - its Max `new` makes the output a one dimensional, 256 cell long matrix (or the type and size
//!   its `planecount type dim` arguments give) after the MOP is set up; median runs no code there,
//!   so the output keeps what the MOP's own setup makes of the class's output settings
//!   (long or char, at least 256 wide, not linked to the input);
//! - `autoclear` and `normalize` are int attributes (median has no char attributes), without
//!   `basic`;
//! - `autoclear` zeroes the output's cells itself instead of sending it `clear`;
//! - for an input of more than two dimensions the C steps through the output with the input's
//!   higher strides; the port counts every input cell into the output's first row;
//! - the accessors run on the caller's thread (the C's defer them, `JIT_ATTR_GET_DEFER_LOW`,
//!   `JIT_ATTR_SET_USURP_LOW`).
//!
//! The output settings in `mop_setup` are untested (median-mock has no MOP io objects); `calc` is
//! tested by calling it directly.

use std::ffi::c_long;

use median::{
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix, MatrixData, MatrixDataType, iter::JitEntryType},
    },
    max_sys::{self, t_jit_error_code as err},
    prelude::*,
};

#[derive(JitMatrixObj)]
#[max(name = "jit.median.histogram", no_methods)]
pub struct JitHistogram {
    #[attr(label = "Normalization Value", category = "Behavior")]
    normval: Int64,
    #[attr(label = "Auto-Clear", style = "onoff", category = "Behavior")]
    autoclear: Int64,
    /// 0 off, 2 over all planes together, anything else per plane.
    #[attr(label = "Normalize Histogram", style = "onoff", category = "Behavior")]
    normalize: Int64,
}

impl MatrixOp for JitHistogram {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            normval: Int64::new(0),
            autoclear: Int64::new(0),
            normalize: Int64::new(0),
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }

    fn mop_setup(mop: &mut MOP<'_>) -> JitResult<()> {
        // GAP: MOP io configuration
        // SAFETY: `mop` is a live MOP being configured before registration; `getoutput` returns
        // its output io object (or null), and the atoms are transparent `t_atom`s.
        unsafe {
            let o =
                max_sys::jit_object_method(mop.as_ptr(), sym!("getoutput").as_ptr(), 1 as c_long);
            if o.is_null() {
                return Ok(());
            }
            max_sys::jit_attr_setlong(o, sym!("dimlink").as_ptr(), 0);
            max_sys::jit_attr_setlong(o, sym!("typelink").as_ptr(), 0);
            let mut mindim = [Atom::from(256 as t_atom_long)];
            max_sys::jit_object_method(
                o,
                sym!("mindim").as_ptr(),
                1 as c_long,
                mindim.as_mut_ptr().cast::<max_sys::t_atom>(),
            );
            // The first type is the default.
            let mut types = [Atom::from(sym!("long")), Atom::from(sym!("char"))];
            max_sys::jit_object_method(
                o,
                sym!("types").as_ptr(),
                2 as c_long,
                types.as_mut_ptr().cast::<max_sys::t_atom>(),
            );
        }
        Ok(())
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let input = inputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let out_info = output.lock().info()?;
        let planes = out_info.planes()?;
        let width = out_info.dimensions()?[0] as usize;
        let counts = {
            let mut lock = input.lock();
            let data = lock.data().map_err(|_| err::JIT_ERR_INVALID_INPUT)?;
            let kind = data.info().kind()?;
            if !matches!(kind, MatrixDataType::Char | MatrixDataType::Int32)
                || out_info.kind()? != MatrixDataType::Int32
            {
                return Err(err::JIT_ERR_MISMATCH_TYPE);
            }
            if width < 256 {
                return Err(err::JIT_ERR_MISMATCH_DIM);
            }
            match kind {
                MatrixDataType::Char => count::<u8>(&data, planes, width)?,
                _ => count::<i32>(&data, planes, width)?,
            }
        };
        let mut lock = output.lock();
        let mut data = lock.data().map_err(|_| err::JIT_ERR_INVALID_OUTPUT)?;
        // SAFETY: the output is this matrix_calc call's own; the input borrow ended above, so an
        // in-place output does not alias it.
        let mut chunks = unsafe { data.chunks_mut::<i32>()? };
        if self.autoclear.get() != 0 {
            for mut chunk in chunks.by_ref() {
                chunk.entry_iter_mut().for_each(|cell| cell.fill(0));
            }
            // SAFETY: as above; the chunks of the first pass are gone.
            chunks = unsafe { data.chunks_mut::<i32>()? };
        }
        let Some(mut first) = chunks.next() else {
            return Ok(());
        };
        let mut row: Vec<&mut [i32]> = first.entry_iter_mut().take(width).collect();
        for (cell, counts) in row.iter_mut().zip(counts.chunks(planes)) {
            for (c, n) in cell.iter_mut().zip(counts) {
                *c = c.wrapping_add(*n);
            }
        }
        let normval = self.normval.get().unsigned_abs() as f64;
        match self.normalize.get() {
            0 => {}
            2 => normalize(row.iter_mut().flat_map(|c| c.iter_mut()), normval),
            _ => {
                for p in 0..planes {
                    normalize(row.iter_mut().map(|c| &mut c[p]), normval);
                }
            }
        }
        Ok(())
    }
}

/// How often each value occurs: `width` cells of `planes` counts, value major. Output plane `p`
/// counts input plane `p` modulo the input's planes; long values outside `0..width` are skipped.
fn count<T: JitEntryType + Into<i64>>(
    data: &MatrixData<'_>,
    planes: usize,
    width: usize,
) -> JitResult<Vec<i32>> {
    let in_planes = data.info().planes()?;
    let mut counts = vec![0i32; width * planes];
    // SAFETY: the input stays locked and unwritten for this matrix_calc call.
    for chunk in unsafe { data.chunks::<T>()? } {
        for cell in chunk.entry_iter() {
            for p in 0..planes {
                let v: i64 = cell[p % in_planes].into();
                if (0..width as i64).contains(&v) {
                    let n = &mut counts[v as usize * planes + p];
                    *n = n.wrapping_add(1);
                }
            }
        }
    }
    Ok(counts)
}

/// Scale `cells` so the largest magnitude becomes `normval`, truncating; all zero if either is 0.
fn normalize<'a>(cells: impl Iterator<Item = &'a mut i32>, normval: f64) {
    let mut cells: Vec<&mut i32> = cells.collect();
    let max = cells.iter().map(|c| c.unsigned_abs()).max().unwrap_or(0);
    let scale = if max == 0 { 0.0 } else { normval / max as f64 };
    for c in cells.iter_mut() {
        **c = (**c as f64 * scale) as i32;
    }
}

median::ext_main! { JitHistogram }

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use median::{
        jit::matrix::{MatrixDataType, MatrixInfo},
        max_sys::{t_jit_err, t_jit_error_code as err},
    };
    use median_mock::{
        Atom,
        jit::{MatrixFixture, MatrixList},
    };

    type Calc = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> t_jit_err;

    fn setup(k: &median_mock::Kernel, args: &[Atom]) -> (*mut c_void, *mut c_void, Calc) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.histogram", args);
        assert!(!x.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        // SAFETY: `matrix_calc` is registered with this signature
        (x, core, unsafe { std::mem::transmute::<usize, Calc>(f) })
    }

    fn run(calc: Calc, core: *mut c_void, i: &MatrixFixture, o: &MatrixFixture) -> t_jit_err {
        let ins = MatrixList::new(&[i]);
        let outs = MatrixList::new(&[o]);
        // SAFETY: live core and lists
        unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
    }

    fn fixture(kind: MatrixDataType, planes: usize, dims: &[usize], bytes: &[u8]) -> MatrixFixture {
        let info = MatrixInfo::new(kind, planes, dims).unwrap();
        let mut m = MatrixFixture::new(*info.as_raw());
        m.write_bytes(bytes);
        m
    }

    fn longs(m: &MatrixFixture) -> Vec<i32> {
        m.bytes()
            .chunks(4)
            .map(|b| i32::from_ne_bytes(b.try_into().unwrap()))
            .collect()
    }

    fn long_bytes(v: &[i32]) -> Vec<u8> {
        v.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    #[test]
    fn counts_char_planes_and_accumulates_without_autoclear() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &[]);
        let input = fixture(
            MatrixDataType::Char,
            2,
            &[2, 2],
            &[0, 5, 0, 5, 255, 5, 3, 1],
        );
        let output = fixture(MatrixDataType::Int32, 2, &[256], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(input.lock_state(), 0);
        assert_eq!(output.lock_state(), 0);
        let h = longs(&output);
        let at = |v: usize, p: usize| h[v * 2 + p];
        assert_eq!((at(0, 0), at(3, 0), at(255, 0)), (2, 1, 1));
        assert_eq!((at(5, 1), at(1, 1)), (3, 1));
        assert_eq!(h.iter().sum::<i32>(), 8);

        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(longs(&output)[0], 4);
        k.attr_set(x, "autoclear", &[Atom::Long(1)]).unwrap();
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(longs(&output)[0], 2);
        assert!(k.free_object(x));
    }

    #[test]
    fn long_values_outside_the_output_are_skipped_and_planes_wrap() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &[]);
        let input = fixture(
            MatrixDataType::Int32,
            1,
            &[4],
            &long_bytes(&[-1, 300, 299, 7]),
        );
        let output = fixture(MatrixDataType::Int32, 2, &[300, 1], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        let h = longs(&output);
        assert_eq!(
            (h[299 * 2], h[299 * 2 + 1], h[7 * 2], h[7 * 2 + 1]),
            (1, 1, 1, 1)
        );
        assert_eq!(h.iter().sum::<i32>(), 4);
        assert!(k.free_object(x));
    }

    #[test]
    fn normalizes_per_plane_or_over_all_planes() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(
            &k,
            &[
                "@normalize".into(),
                1i64.into(),
                "@normval".into(),
                (-100i64).into(),
                "@autoclear".into(),
                1i64.into(),
            ],
        );
        let input = fixture(MatrixDataType::Char, 2, &[3], &[1, 2, 1, 2, 2, 2]);
        let output = fixture(MatrixDataType::Int32, 2, &[256], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        let h = longs(&output);
        // Plane 0: two 1s and a 2; plane 1: three 2s.
        assert_eq!((h[2], h[4], h[5]), (100, 50, 100));
        k.attr_set(x, "normalize", &[Atom::Long(2)]).unwrap();
        assert_eq!(run(calc, core, &input, &output), 0);
        let h = longs(&output);
        assert_eq!((h[2], h[4], h[5]), (66, 33, 100));
        k.attr_set(x, "normval", &[Atom::Long(0)]).unwrap();
        assert_eq!(run(calc, core, &input, &output), 0);
        assert!(longs(&output).iter().all(|&v| v == 0));
        assert!(k.free_object(x));
    }

    #[test]
    fn refuses_float_input_long_mismatch_and_narrow_output() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k, &[]);
        let char_in = fixture(MatrixDataType::Char, 1, &[2], &[1, 2]);
        let float_in = fixture(MatrixDataType::Float32, 1, &[2], &[0; 8]);
        let out = fixture(MatrixDataType::Int32, 1, &[256], &[]);
        let char_out = fixture(MatrixDataType::Char, 1, &[256], &[]);
        let narrow = fixture(MatrixDataType::Int32, 1, &[255], &[]);
        let code = |i, o| run(calc, core, i, o);
        assert_eq!(
            code(&float_in, &out),
            err::JIT_ERR_MISMATCH_TYPE as t_jit_err
        );
        assert_eq!(
            code(&char_in, &char_out),
            err::JIT_ERR_MISMATCH_TYPE as t_jit_err
        );
        assert_eq!(
            code(&char_in, &narrow),
            err::JIT_ERR_MISMATCH_DIM as t_jit_err
        );
        assert_eq!(float_in.lock_state(), 0);
        assert_eq!(narrow.lock_state(), 0);
        assert!(k.free_object(x));
    }
}
