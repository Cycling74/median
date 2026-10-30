//! A port of the SDK's `matrix/jit.concat`: joins two matrices along one dimension.
//!
//! The output is resized, cleared and filled with raw `setinfo`, `clear` and `frommatrix` calls
//! on the matrices (no median API resizes or copies between matrices). They are made with no
//! matrix locked by median, so no Rust view of matrix data is alive across them. Input 2 and the
//! output are unlinked from input 1 with raw MOP calls, and input 2 keeps its own size
//! (`jit_mop_ioproc_copy_adapt`); median-mock has none of those functions, so the tests build
//! without them, and the mock refuses the matrix methods, so the tests check the layout and that
//! `calc` reaches each call, not the copied data.
//!
//! Deviations: `char` attributes are `Int64`, none is basic; a dimension past a matrix's
//! `dimcount` counts as 1 (the C reads whatever its info holds there); the output is also resized
//! when only the type differs.

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
#[max(name = "jit.median.concat", no_methods)]
pub struct Concat {
    // GAP: CLASS_ATTR_CHAR
    #[attr(
        label = "Concatenation Order Mode",
        enum = "\"Append Right to Left\" \"Append Left to Right\"",
        category = "Behavior"
    )]
    mode: Int64,
    #[attr(label = "Dimension to Concatenate", category = "Behavior")]
    concatdim: Int64,
    #[attr(label = "Automatically Clear", style = "onoff", category = "Behavior")]
    autoclear: Int64,
    #[attr(label = "Truncate", style = "onoff", category = "Behavior")]
    truncate: Int64,
}

impl MatrixOp for Concat {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            mode: Int64::new(0),
            concatdim: Int64::new(0),
            autoclear: Int64::new(1),
            truncate: Int64::new(1),
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(2),
            outputs: Count::Fixed(1),
        }
    }

    fn mop_setup(_mop: &mut MOP<'_>) -> JitResult<()> {
        // Not in tests: median-mock lacks these functions, and calling one aborts the test.
        #[cfg(not(test))]
        // SAFETY: `_mop` is this class's MOP, configured before registration; input 2 exists.
        // GAP: jit_mop_input_nolink, jit_mop_output_nolink, MOP io `ioproc`
        unsafe {
            jit_ok(max_sys::jit_mop_input_nolink(_mop.as_ptr(), 2))?;
            jit_ok(max_sys::jit_mop_output_nolink(_mop.as_ptr(), 1))?;
            let input =
                max_sys::jit_object_method(_mop.as_ptr(), sym!("getinput").as_ptr(), 2 as c_long);
            if input.is_null() {
                return Err(err::JIT_ERR_INVALID_PTR);
            }
            max_sys::jit_object_method(
                input,
                sym!("ioproc").as_ptr(),
                max_sys::jit_mop_ioproc_copy_adapt as *mut std::ffi::c_void,
            );
        }
        Ok(())
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let ([in1, in2], [out]) = (inputs, outputs) else {
            return Err(err::JIT_ERR_INVALID_PTR);
        };
        let (info1, info2, out_info) = (info(in1)?, info(in2)?, info(out)?);
        let (want, copies) = layout(self.settings(), &info1, info2, &out_info);
        // SAFETY: the matrices are live for this call, and no guard is alive, so no view of
        // their data is.
        unsafe {
            if resized(&want, &out_info) && setinfo(out, &want) {
                return Err(err::JIT_ERR_GENERIC);
            }
            if self.autoclear.get() != 0 {
                clear(out);
            }
            for (source, conv) in copies {
                frommatrix(out, if source == 0 { in1 } else { in2 }, &conv);
            }
        }
        Ok(())
    }
}

impl Concat {
    fn settings(&self) -> Settings {
        Settings {
            // The C clamps to 32, one past the last dimension.
            concatdim: self.concatdim.get().clamp(0, MAX_DIMS as isize - 1) as usize,
            right_first: self.mode.get() != 0,
            truncate: self.truncate.get() != 0,
        }
    }
}

#[derive(Clone, Copy)]
struct Settings {
    concatdim: usize,
    right_first: bool,
    truncate: bool,
}

/// The output's info, and each input copy as (input index, ranges) in the order they are made.
fn layout(
    s: Settings,
    in1: &t_jit_matrix_info,
    mut in2: t_jit_matrix_info,
    out: &t_jit_matrix_info,
) -> (t_jit_matrix_info, [(usize, ConvInfo); 2]) {
    let cd = s.concatdim;
    if s.truncate {
        for i in (0..in2.dimcount as usize).filter(|&i| i != cd) {
            in2.dim[i] = in2.dim[i].min(dim(in1, i));
        }
    }
    let mut want = *out;
    want.planecount = in1.planecount.max(in2.planecount);
    want.type_ = in1.type_;
    want.dimcount = in1.dimcount.max(in2.dimcount).max(cd as c_long + 1);
    for i in 0..want.dimcount as usize {
        want.dim[i] = if i == cd {
            dim(in1, i) + dim(&in2, i)
        } else {
            dim(in1, i).max(dim(&in2, i))
        };
    }
    let ((ia, a), (ib, b)) = if s.right_first {
        ((1, &in2), (0, in1))
    } else {
        ((0, in1), (1, &in2))
    };
    let (mut first, mut second) = (ConvInfo::new(), ConvInfo::new());
    for i in 0..want.dimcount as usize {
        let (da, db) = (dim(a, i), dim(b, i));
        first.srcdimend[i] = da - 1;
        first.dstdimend[i] = da - 1;
        second.srcdimend[i] = db - 1;
        if i == cd {
            second.dstdimstart[i] = da;
            second.dstdimend[i] = da + db - 1;
        } else {
            second.dstdimend[i] = db - 1;
        }
    }
    (want, [(ia, first), (ib, second)])
}

fn dim(info: &t_jit_matrix_info, i: usize) -> c_long {
    if (i as c_long) < info.dimcount {
        info.dim[i]
    } else {
        1
    }
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

median::ext_main! { Concat }

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

    const DEFAULT: Settings = Settings {
        concatdim: 0,
        right_first: false,
        truncate: true,
    };

    #[test]
    fn appends_the_right_input_along_the_dimension() {
        let _k = median_mock::lock();
        let a = raw(MatrixDataType::Char, 4, &[3, 2]);
        let b = raw(MatrixDataType::Char, 1, &[2, 2]);
        let (want, [(i, first), (j, second)]) = layout(DEFAULT, &a, b, &b);
        assert_eq!((want.planecount, want.dimcount), (4, 2));
        assert_eq!(&want.dim[..2], &[5, 2]);
        assert_eq!(want.type_, a.type_);
        assert_eq!((i, j), (0, 1));
        assert_eq!(ranges(&first, 2), [[0, 2, 0, 2], [0, 1, 0, 1]]);
        assert_eq!(ranges(&second, 2), [[0, 1, 3, 4], [0, 1, 0, 1]]);
        assert_eq!(first.planemap[31], 31);

        let right_first = Settings {
            right_first: true,
            ..DEFAULT
        };
        let (_, [(i, first), (j, second)]) = layout(right_first, &a, b, &b);
        assert_eq!((i, j), (1, 0));
        assert_eq!(ranges(&first, 2), [[0, 1, 0, 1], [0, 1, 0, 1]]);
        assert_eq!(ranges(&second, 2), [[0, 2, 2, 4], [0, 1, 0, 1]]);
    }

    #[test]
    fn truncates_other_dimensions_to_the_left_input() {
        let _k = median_mock::lock();
        let a = raw(MatrixDataType::Float32, 1, &[2, 2]);
        let b = raw(MatrixDataType::Float32, 1, &[2, 3]);
        let (want, [_, (_, second)]) = layout(DEFAULT, &a, b, &a);
        assert_eq!(&want.dim[..2], &[4, 2]);
        assert_eq!(ranges(&second, 2)[1], [0, 1, 0, 1]);
        let whole = Settings {
            truncate: false,
            ..DEFAULT
        };
        let (want, [_, (_, second)]) = layout(whole, &a, b, &a);
        assert_eq!(&want.dim[..2], &[4, 3]);
        assert_eq!(ranges(&second, 2)[1], [0, 2, 0, 2]);

        // A new dimension: two 1D matrices side by side in dimension 1.
        let a = raw(MatrixDataType::Char, 1, &[4]);
        let up = Settings {
            concatdim: 1,
            ..DEFAULT
        };
        let (want, [(_, first), (_, second)]) = layout(up, &a, a, &a);
        assert_eq!(want.dimcount, 2);
        assert_eq!(&want.dim[..2], &[4, 2]);
        assert_eq!(ranges(&first, 2), [[0, 3, 0, 3], [0, 0, 0, 0]]);
        assert_eq!(ranges(&second, 2), [[0, 3, 0, 3], [0, 0, 1, 1]]);
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
    fn calc_resizes_clears_and_copies_both_inputs() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.concat", &["@concatdim".into(), 1i64.into()]);
        assert!(!x.is_null(), "{:?}", k.console_log());
        assert_eq!(k.attr_get(x, "concatdim").unwrap(), [MockAtom::Long(1)]);
        assert_eq!(k.attr_get(x, "autoclear").unwrap(), [MockAtom::Long(1)]);
        assert_eq!(k.attr_get(x, "truncate").unwrap(), [MockAtom::Long(1)]);
        assert_eq!(k.attr_get(x, "mode").unwrap(), [MockAtom::Long(0)]);
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        // SAFETY: the registered `matrix_calc` has this signature.
        let calc: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
        ) -> max_sys::t_jit_err = unsafe { std::mem::transmute(f) };
        let a = MatrixFixture::new(raw(MatrixDataType::Char, 1, &[2, 2]));
        let b = MatrixFixture::new(raw(MatrixDataType::Char, 1, &[2, 2]));
        let run = |out: &MatrixFixture| {
            let ins = MatrixList::new(&[&a, &b]);
            let outs = MatrixList::new(&[out]);
            // SAFETY: the lists hold live matrices for the call.
            unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
        };
        let small = MatrixFixture::new(raw(MatrixDataType::Char, 1, &[2, 2]));
        k.take_console_log();
        assert_eq!(run(&small), 0);
        assert_eq!(
            unsupported(&k),
            ["setinfo", "clear", "frommatrix", "frommatrix"]
        );
        k.attr_set(x, "autoclear", &[MockAtom::Long(0)]).unwrap();
        let sized = MatrixFixture::new(raw(MatrixDataType::Char, 1, &[2, 4]));
        assert_eq!(run(&sized), 0);
        assert_eq!(unsupported(&k), ["frommatrix", "frommatrix"]);
        assert_eq!(sized.lock_state(), 0);
        assert!(k.free_object(x));
    }
}
