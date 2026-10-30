//! A port of the SDK's `matrix/jit.split`: splits a matrix in two at a point along one dimension.
//!
//! The outputs are resized, cleared and filled with raw `setinfo`, `clear` and `frommatrix` calls
//! on the matrices (no median API resizes or copies between matrices). They are made with no
//! matrix locked by median, so no Rust view of matrix data is alive across them. Both outputs
//! are unlinked from the input with raw MOP calls; median-mock has neither function, so the tests
//! build without them, and the mock refuses the matrix methods, so the tests check the layout and
//! that `calc` reaches each call, not the copied data.
//!
//! Deviation: `char` attributes are `Int64`.

use std::ffi::c_long;

use median::{
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix},
    },
    max_sys::{self, t_jit_error_code as err, t_jit_matrix_info},
    prelude::*,
    sym,
};

const MAX_DIMS: usize = 32;

#[derive(JitMatrixObj)]
#[max(name = "jit.median.split", no_methods)]
pub struct Split {
    // GAP: CLASS_ATTR_CHAR
    #[attr(label = "Split Dim")]
    splitdim: Int64,
    #[attr(label = "Auto Clear")]
    autoclear: Int64,
    #[attr(label = "Split Point")]
    splitpoint: Int64,
}

impl MatrixOp for Split {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            splitdim: Int64::new(0),
            autoclear: Int64::new(1),
            splitpoint: Int64::new(1),
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(2),
        }
    }

    fn mop_setup(_mop: &mut MOP<'_>) -> JitResult<()> {
        // Not in tests: median-mock lacks these functions, and calling one aborts the test.
        #[cfg(not(test))]
        // SAFETY: `_mop` is this class's MOP, configured before registration; outputs 1 and 2
        // exist.
        // GAP: jit_mop_output_nolink
        unsafe {
            jit_ok(max_sys::jit_mop_output_nolink(_mop.as_ptr(), 1))?;
            jit_ok(max_sys::jit_mop_output_nolink(_mop.as_ptr(), 2))?;
        }
        Ok(())
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let ([input], [out1, out2]) = (inputs, outputs) else {
            return Err(err::JIT_ERR_INVALID_PTR);
        };
        let (in_info, info1, info2) = (info(input)?, info(out1)?, info(out2)?);
        // The C clamps to 32, one past the last dimension.
        let splitdim = self.splitdim.get().clamp(0, MAX_DIMS as isize - 1) as usize;
        let [(want1, conv1), (want2, conv2)] = layout(
            splitdim,
            self.splitpoint.get() as c_long,
            &in_info,
            [&info1, &info2],
        );
        // SAFETY: the matrices are live for this call, and no guard is alive, so no view of
        // their data is.
        unsafe {
            if (resized(&want1, &info1) || resized(&want2, &info2))
                && (setinfo(out1, &want1) || setinfo(out2, &want2))
            {
                return Err(err::JIT_ERR_GENERIC);
            }
            if self.autoclear.get() != 0 {
                clear(out1);
                clear(out2);
            }
            frommatrix(out1, input, &conv1);
            frommatrix(out2, input, &conv2);
        }
        Ok(())
    }
}

/// Each output's info and the ranges copied into it.
fn layout(
    splitdim: usize,
    splitpoint: c_long,
    input: &t_jit_matrix_info,
    outs: [&t_jit_matrix_info; 2],
) -> [(t_jit_matrix_info, ConvInfo); 2] {
    let mut parts = outs.map(|out| {
        let mut want = *out;
        want.planecount = input.planecount;
        want.type_ = input.type_;
        want.dimcount = input.dimcount;
        (want, ConvInfo::new())
    });
    for i in 0..input.dimcount as usize {
        let n = input.dim[i];
        for (want, conv) in &mut parts {
            want.dim[i] = n;
            conv.srcdimend[i] = n - 1;
            conv.dstdimend[i] = n - 1;
        }
        if i == splitdim {
            let [(want1, conv1), (want2, conv2)] = &mut parts;
            want1.dim[i] = clamp(splitpoint, 0, n);
            want2.dim[i] = clamp(n - splitpoint, 0, n);
            conv1.srcdimend[i] = clamp(splitpoint - 1, 0, n - 1);
            conv1.dstdimend[i] = want1.dim[i] - 1;
            conv2.srcdimstart[i] = clamp(splitpoint, 0, n - 1);
            conv2.dstdimend[i] = want2.dim[i] - 1;
        }
    }
    parts
}

/// `CLAMP` as `ext_common.h` defines it: `lo` wins when `hi < lo`.
fn clamp(a: c_long, lo: c_long, hi: c_long) -> c_long {
    if a > lo { a.min(hi) } else { lo }
}

fn resized(want: &t_jit_matrix_info, have: &t_jit_matrix_info) -> bool {
    let n = want.dimcount as usize;
    want.type_ != have.type_
        || want.planecount != have.planecount
        || want.dimcount != have.dimcount
        || want.dim[..n] != have.dim[..n]
}

fn info(m: &mut Matrix<'_>) -> JitResult<t_jit_matrix_info> {
    Ok(*m.lock().info()?.as_raw())
}

#[cfg(not(test))]
fn jit_ok(code: max_sys::t_jit_err) -> JitResult<()> {
    if code == 0 { Ok(()) } else { Err(code as _) }
}

/// `t_matrix_conv_info` (`jit.common.h`), which max-sys does not bind.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct ConvInfo {
    flags: c_long,
    planemap: [c_long; 32],
    srcdimstart: [c_long; MAX_DIMS],
    srcdimend: [c_long; MAX_DIMS],
    dstdimstart: [c_long; MAX_DIMS],
    dstdimend: [c_long; MAX_DIMS],
}

impl ConvInfo {
    /// Copies cells between the given ranges, each plane to the same plane.
    fn new() -> Self {
        use max_sys::t_matrix_conv_info_flags::*;
        Self {
            flags: (JIT_MATRIX_CONVERT_SRCDIM | JIT_MATRIX_CONVERT_DSTDIM) as c_long,
            planemap: std::array::from_fn(|i| i as c_long),
            srcdimstart: [0; MAX_DIMS],
            srcdimend: [0; MAX_DIMS],
            dstdimstart: [0; MAX_DIMS],
            dstdimend: [0; MAX_DIMS],
        }
    }
}

// GAP: matrix setinfo, clear, frommatrix
/// Resizes `m`; true if the matrix refused.
///
/// # Safety
/// No view of any matrix's data may be alive: resizing reallocates it.
unsafe fn setinfo(m: &Matrix<'_>, info: &t_jit_matrix_info) -> bool {
    // SAFETY: `m` is live; `info` is a complete matrix info the matrix only reads.
    !unsafe { max_sys::jit_object_method(m.as_ptr(), sym!("setinfo").as_ptr(), info as *const _) }
        .is_null()
}

/// # Safety
/// No view of `m`'s data may be alive.
unsafe fn clear(m: &Matrix<'_>) {
    // SAFETY: `m` is live.
    unsafe { max_sys::jit_object_method(m.as_ptr(), sym!("clear").as_ptr()) };
}

/// Copies the ranges of `conv` from `src` into `dst`.
///
/// # Safety
/// No view of either matrix's data may be alive.
unsafe fn frommatrix(dst: &Matrix<'_>, src: &Matrix<'_>, conv: &ConvInfo) {
    // SAFETY: both are live; `conv` is a `t_matrix_conv_info` the matrix only reads.
    unsafe {
        max_sys::jit_object_method(
            dst.as_ptr(),
            sym!("frommatrix").as_ptr(),
            src.as_ptr(),
            conv as *const ConvInfo,
        )
    };
}

median::ext_main! { Split }

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_void;
    use median::jit::matrix::{MatrixDataType, MatrixInfo};
    use median_mock::{
        Atom as MockAtom,
        jit::{MatrixFixture, MatrixList},
    };

    fn raw(kind: MatrixDataType, planes: usize, dims: &[usize]) -> t_jit_matrix_info {
        *MatrixInfo::new(kind, planes, dims).unwrap().as_raw()
    }

    fn ranges(c: &ConvInfo, n: usize) -> Vec<[c_long; 4]> {
        (0..n)
            .map(|i| {
                [
                    c.srcdimstart[i],
                    c.srcdimend[i],
                    c.dstdimstart[i],
                    c.dstdimend[i],
                ]
            })
            .collect()
    }

    #[test]
    fn splits_at_the_point_and_clamps_it() {
        let _k = median_mock::lock();
        let input = raw(MatrixDataType::Float64, 3, &[4, 2]);
        let other = raw(MatrixDataType::Char, 1, &[1]);
        let [(want1, conv1), (want2, conv2)] = layout(0, 1, &input, [&other, &other]);
        for want in [&want1, &want2] {
            assert_eq!((want.planecount, want.dimcount), (3, 2));
            assert_eq!(want.type_, input.type_);
        }
        assert_eq!(&want1.dim[..2], &[1, 2]);
        assert_eq!(&want2.dim[..2], &[3, 2]);
        assert_eq!(ranges(&conv1, 2), [[0, 0, 0, 0], [0, 1, 0, 1]]);
        assert_eq!(ranges(&conv2, 2), [[1, 3, 0, 2], [0, 1, 0, 1]]);

        let [(want1, conv1), (want2, conv2)] = layout(1, 5, &input, [&other, &other]);
        assert_eq!(&want1.dim[..2], &[4, 2]);
        assert_eq!(&want2.dim[..2], &[4, 0]);
        assert_eq!(ranges(&conv1, 2)[1], [0, 1, 0, 1]);
        assert_eq!(ranges(&conv2, 2)[1], [1, 1, 0, -1]);

        // A split dimension the input does not have copies it whole to both.
        let [(want1, _), (want2, conv2)] = layout(2, 1, &input, [&other, &other]);
        assert_eq!(&want1.dim[..2], &[4, 2]);
        assert_eq!(&want2.dim[..2], &[4, 2]);
        assert_eq!(ranges(&conv2, 2), [[0, 3, 0, 3], [0, 1, 0, 1]]);
    }

    fn unsupported(k: &median_mock::Kernel) -> Vec<String> {
        k.take_console_log()
            .into_iter()
            .filter_map(|e| {
                e.text
                    .strip_prefix("median-mock: unsupported Jitter method ")
                    .map(str::to_owned)
            })
            .collect()
    }

    #[test]
    fn calc_resizes_clears_and_copies_into_both_outputs() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.split", &["@splitpoint".into(), 2i64.into()]);
        assert!(!x.is_null(), "{:?}", k.console_log());
        assert_eq!(k.attr_get(x, "splitpoint").unwrap(), [MockAtom::Long(2)]);
        assert_eq!(k.attr_get(x, "splitdim").unwrap(), [MockAtom::Long(0)]);
        assert_eq!(k.attr_get(x, "autoclear").unwrap(), [MockAtom::Long(1)]);
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        // SAFETY: the registered `matrix_calc` has this signature.
        let calc: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
        ) -> max_sys::t_jit_err = unsafe { std::mem::transmute(f) };
        let input = MatrixFixture::new(raw(MatrixDataType::Char, 1, &[4, 2]));
        let run = |out1: &MatrixFixture, out2: &MatrixFixture| {
            let ins = MatrixList::new(&[&input]);
            let outs = MatrixList::new(&[out1, out2]);
            // SAFETY: the lists hold live matrices for the call.
            unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
        };
        let half = MatrixFixture::new(raw(MatrixDataType::Char, 1, &[2, 2]));
        let whole = MatrixFixture::new(raw(MatrixDataType::Char, 1, &[4, 2]));
        k.take_console_log();
        assert_eq!(run(&half, &whole), 0);
        assert_eq!(
            unsupported(&k),
            [
                "setinfo",
                "setinfo",
                "clear",
                "clear",
                "frommatrix",
                "frommatrix"
            ]
        );
        k.attr_set(x, "autoclear", &[MockAtom::Long(0)]).unwrap();
        assert_eq!(run(&half, &half), 0);
        assert_eq!(unsupported(&k), ["frommatrix", "frommatrix"]);
        assert_eq!(input.lock_state(), 0);
        assert!(k.free_object(x));
    }
}
