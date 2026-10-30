//! A port of the SDK's `matrix/jit.rgb2luma`: converts a 4-plane ARGB char matrix to 1-plane
//! luminance with a weighted sum of the planes.
//!
//! The C calculates in parallel (`jit_parallel_ndim_simplecalc2`); this port calculates serially.
//! The C's Max wrapper also sets the output matrix to 1-plane, 2-dimensional char right after
//! `max_jit_mop_setup_simple`; median's Max side has no hook there, so the port relies on the MOP
//! (one char plane on every io, `planelink 0` on the output) to shape the output. The output's
//! `planelink` is set with a raw call that median-mock answers with null, so it is untested.

use core::ffi::c_long;
use median::{
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix, MatrixDataType},
    },
    max_sys::{self, t_jit_error_code as err},
    prelude::*,
};

#[derive(JitMatrixObj)]
#[max(name = "jit.median.rgb2luma", no_methods)]
pub struct Rgb2Luma {
    #[attr(label = "Alpha Scale")]
    ascale: Float64,
    #[attr(label = "Red Scale")]
    rscale: Float64,
    #[attr(label = "Green Scale")]
    gscale: Float64,
    #[attr(label = "Blue Scale")]
    bscale: Float64,
}

impl MatrixOp for Rgb2Luma {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            ascale: 0.0.into(),
            rscale: 0.299.into(),
            gscale: 0.587.into(),
            bscale: 0.114.into(),
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }

    fn mop_setup(mop: &mut MOP<'_>) -> JitResult<()> {
        mop.single_plane_count(1)?;
        mop.single_type(MatrixDataType::Char)?;
        // SAFETY: `mop` is a live MOP being configured during registration.
        // GAP: per-io MOP configuration
        unsafe {
            let output =
                max_sys::jit_object_method(mop.as_ptr(), sym!("getoutput").as_ptr(), 1 as c_long);
            if !output.is_null() {
                let off = Atom::from(0i64);
                max_sys::jit_object_method(
                    output,
                    sym!("planelink").as_ptr(),
                    1 as c_long,
                    &raw const off,
                );
            }
        }
        Ok(())
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let input = inputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let (input_dims, pixels) = {
            let mut lock = input.lock();
            let data = lock.data().map_err(|_| err::JIT_ERR_GENERIC)?;
            let info = *data.info();
            if info.kind()? != MatrixDataType::Char {
                return Err(err::JIT_ERR_MISMATCH_TYPE);
            }
            if info.planes()? != 4 {
                return Err(err::JIT_ERR_MISMATCH_PLANE);
            }
            let dims = dims(info.dimensions()?);
            let mut pixels = Vec::new();
            // SAFETY: the input is stable and unwritten during matrix_calc. It is copied before
            // any output borrow, so an in-place output cannot alias it.
            for chunk in unsafe { data.chunks::<u8>()? } {
                for cell in chunk.entry_iter() {
                    pixels.push([cell[0], cell[1], cell[2], cell[3]]);
                }
            }
            (dims, pixels)
        };
        let scales = [&self.ascale, &self.rscale, &self.gscale, &self.bscale]
            .map(|s| (s.get() * 65536.0) as i64);
        let mut lock = output.lock();
        let mut data = lock.data().map_err(|_| err::JIT_ERR_GENERIC)?;
        let info = *data.info();
        if info.kind()? != MatrixDataType::Char {
            return Err(err::JIT_ERR_MISMATCH_TYPE);
        }
        if info.planes()? != 1 {
            return Err(err::JIT_ERR_MISMATCH_PLANE);
        }
        let out_dims = dims(info.dimensions()?);
        let mut linear = 0;
        // SAFETY: the output belongs to this matrix_calc call and no input borrow is alive.
        for mut chunk in unsafe { data.chunks_mut::<u8>()? } {
            for cell in chunk.entry_iter_mut() {
                if let Some(source) = source_index(linear, &out_dims, &input_dims) {
                    let sum = pixels[source]
                        .iter()
                        .zip(scales)
                        .fold(0i64, |acc, (&p, s)| acc + p as i64 * s);
                    cell[0] = (sum >> 16).clamp(0, 255) as u8;
                }
                linear += 1;
            }
        }
        Ok(())
    }
}

fn dims(raw: &[c_long]) -> Vec<usize> {
    raw.iter().map(|&d| d as usize).collect()
}

/// The input cell for output cell `linear` (dimension 0 fastest), or `None` outside the input.
fn source_index(mut linear: usize, out_dims: &[usize], in_dims: &[usize]) -> Option<usize> {
    let (mut source, mut stride) = (0, 1);
    for (i, &dim) in out_dims.iter().enumerate() {
        let index = linear % dim;
        linear /= dim;
        let in_dim = in_dims.get(i).copied().unwrap_or(1);
        if index >= in_dim {
            return None;
        }
        source += index * stride;
        stride *= in_dim;
    }
    Some(source)
}

median::ext_main! { Rgb2Luma }

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

    fn run(calc: Calc, core: *mut c_void, input: &MatrixFixture, output: &MatrixFixture) -> i64 {
        let ins = MatrixList::new(&[input]);
        let outs = MatrixList::new(&[output]);
        unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
    }

    #[test]
    fn weights_argb_into_one_plane() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.rgb2luma", &[]);
        assert!(!x.is_null(), "{:?}", k.console_log());
        assert_eq!(k.attr_get(x, "gscale").unwrap(), [Atom::Float(0.587)]);
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        let calc: Calc = unsafe { std::mem::transmute(f) };

        let input = fixture(4, &[2, 1], &[9, 255, 255, 255, 255, 100, 0, 0]);
        let output = fixture(1, &[2, 1], &[0, 0]);
        assert_eq!(run(calc, core, &input, &output), 0);
        // 0.299 * 65536 truncates to 19595: (255 * 65535) >> 16 = 254, (100 * 19595) >> 16 = 29.
        assert_eq!(output.bytes(), [254, 29]);
        assert_eq!((input.lock_state(), output.lock_state()), (0, 0));

        // Out of the safe range the sum is clamped.
        k.attr_set(x, "ascale", &[Atom::Float(-1.0)]).unwrap();
        k.attr_set(x, "rscale", &[Atom::Float(2.0)]).unwrap();
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), [255, 0]);

        // Only the intersection is written.
        let wide = fixture(1, &[3, 1], &[7, 7, 7]);
        assert_eq!(run(calc, core, &input, &wide), 0);
        assert_eq!(wide.bytes(), [255, 0, 7]);

        let three = fixture(3, &[2, 1], &[0; 6]);
        assert_ne!(run(calc, core, &three, &output), 0);
        assert_ne!(run(calc, core, &input, &input), 0);
        assert!(k.free_object(x));
    }

    #[test]
    fn has_mop_iolets_and_labelled_attributes() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.rgb2luma", &["@bscale".into(), 0.5.into()]);
        assert_eq!(k.attr_get(x, "bscale").unwrap(), [Atom::Float(0.5)]);
        assert_eq!(k.outlet_count(x), 2);
        let core = median_mock::jit::object_for(x);
        let (_, label) = k.attr_attr(core, "rscale", "label").unwrap();
        assert_eq!(label, [Atom::Sym("Red Scale".into())]);
        assert!(k.free_object(x));
    }
}
