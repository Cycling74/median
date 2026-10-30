//! A port of the SDK's `matrix/jit.demultiplex`: deals runs of `scan_a` and `scan_b` slices of a
//! matrix along one dimension alternately to the left and right outputs.
//!
//! The outputs are resized, cleared and filled with raw `setinfo`, `clear` and `frommatrix` calls
//! on the matrices (no median API resizes or copies between matrices). They are made with no
//! matrix locked by median, so no Rust view of matrix data is alive across them. Both outputs
//! are unlinked from the input with raw MOP calls; median-mock has neither function, so the tests
//! build without them, and the mock refuses the matrix methods, so the tests check the layout and
//! that `calc` reaches each call, not the copied data.
//!
//! Deviations: `char` attributes are `Int64`, none is basic; a dimension past the input's
//! `dimcount` counts as 1 (the C reads whatever its info holds there); the outputs are also
//! resized when only the type differs.

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
#[max(name = "jit.median.demultiplex", no_methods)]
pub struct Demultiplex {
    // GAP: CLASS_ATTR_CHAR
    #[attr(
        label = "Dimension to Demultiplex",
        enum = "0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31",
        category = "Behavior"
    )]
    demultiplexdim: Int64,
    #[attr(label = "Auto-Clear", style = "onoff", category = "Behavior")]
    autoclear: Int64,
    #[attr(label = "Scanline Count (Left)", category = "Behavior")]
    scan_a: Int64,
    #[attr(label = "Scanline Count (Right)", category = "Behavior")]
    scan_b: Int64,
}

impl MatrixOp for Demultiplex {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            demultiplexdim: Int64::new(0),
            autoclear: Int64::new(1),
            scan_a: Int64::new(1),
            scan_b: Int64::new(1),
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
        let dmd = self.demultiplexdim.get().clamp(0, MAX_DIMS as isize - 1) as usize;
        let (want1, want2, copies) = layout(
            dmd,
            self.scan_a.get() as c_long,
            self.scan_b.get() as c_long,
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
            for (output, conv) in copies {
                frommatrix(if output == 0 { out1 } else { out2 }, input, &conv);
            }
        }
        Ok(())
    }
}

/// The outputs' infos, and each copy as (output index, ranges) in the order they are made.
fn layout(
    dmd: usize,
    scan_a: c_long,
    scan_b: c_long,
    input: &t_jit_matrix_info,
    outs: [&t_jit_matrix_info; 2],
) -> (t_jit_matrix_info, t_jit_matrix_info, Vec<(usize, ConvInfo)>) {
    let dc = input.dimcount as usize;
    let [mut want1, mut want2] = outs.map(|out| {
        let mut want = *out;
        want.planecount = input.planecount;
        want.type_ = input.type_;
        want.dimcount = input.dimcount;
        want.dim[..dc].copy_from_slice(&input.dim[..dc]);
        want
    });
    let mut conv1 = ConvInfo::new();
    for i in 0..dc {
        conv1.srcdimend[i] = input.dim[i] - 1;
        conv1.dstdimend[i] = input.dim[i] - 1;
    }
    let mut conv2 = conv1;

    let i = dmd;
    let n = dim(input, i);
    let scan_a = clamp(scan_a, 0, n);
    let scan_b = clamp(scan_b, 0, n);
    let mut copies = Vec::new();
    // The C divides by `scan_a + scan_b` before these two cases, by zero when both are 0.
    if scan_b == 0 {
        want1.dim[i] = scan_a;
        want2.dim[i] = 0;
        set(&mut conv1, i, n, 0, 0, scan_a, want1.dim[i]);
        copies.push((0, conv1));
        return (want1, want2, copies);
    }
    if scan_a == 0 {
        want1.dim[i] = 0;
        want2.dim[i] = scan_b;
        set(&mut conv2, i, n, 0, 0, scan_b, want2.dim[i]);
        copies.push((1, conv2));
        return (want1, want2, copies);
    }
    let plexsize = clamp(scan_a + scan_b, 0, n);
    let (runs, rest) = (n / plexsize, n % plexsize);
    let (offset, offset2) = if rest != 0 {
        (rest.min(scan_a), (rest - scan_a).max(0))
    } else {
        (0, 0)
    };
    want1.dim[i] = scan_a * runs + offset;
    want2.dim[i] = scan_b * runs + offset2;
    let (d1, d2) = (want1.dim[i], want2.dim[i]);
    let (mut a_start, mut b_start, mut c_start) = (0, 0, 0);
    for _ in 0..runs {
        set(&mut conv1, i, n, a_start, b_start, scan_a, d1);
        a_start += scan_a;
        b_start += scan_a;
        set(&mut conv2, i, n, a_start, c_start, scan_b, d2);
        a_start += scan_b;
        c_start += scan_b;
        copies.push((0, conv1));
        copies.push((1, conv2));
    }
    if rest != 0 {
        set(&mut conv1, i, n, a_start, b_start, offset, d1);
        a_start += offset;
        copies.push((0, conv1));
        if a_start < n {
            set(&mut conv2, i, n, a_start, c_start, offset2, d2);
            copies.push((1, conv2));
        }
    }
    (want1, want2, copies)
}

/// Sets dimension `i` of `conv` to copy `count` slices from `src` of `n` to `dst` of `out_n`,
/// clamping both ranges as the C does.
fn set(
    conv: &mut ConvInfo,
    i: usize,
    n: c_long,
    src: c_long,
    dst: c_long,
    count: c_long,
    out_n: c_long,
) {
    conv.srcdimstart[i] = clamp(src, 0, n - 1);
    conv.srcdimend[i] = clamp(src + count - 1, 0, n - 1);
    conv.dstdimstart[i] = clamp(dst, 0, out_n - 1);
    conv.dstdimend[i] = clamp(dst + count - 1, 0, out_n - 1);
}

/// `CLAMP` as `ext_common.h` defines it: `lo` wins when `hi < lo`.
fn clamp(a: c_long, lo: c_long, hi: c_long) -> c_long {
    if a > lo { a.min(hi) } else { lo }
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

median::ext_main! { Demultiplex }

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
    fn deals_alternating_runs() {
        let _k = median_mock::lock();
        let input = raw(MatrixDataType::Char, 2, &[3, 5]);
        let other = raw(MatrixDataType::Float32, 1, &[1]);
        let (want1, want2, copies) = layout(1, 1, 1, &input, [&other, &other]);
        for want in [&want1, &want2] {
            assert_eq!((want.planecount, want.dimcount), (2, 2));
            assert_eq!(want.type_, input.type_);
        }
        assert_eq!(&want1.dim[..2], &[3, 3]);
        assert_eq!(&want2.dim[..2], &[3, 2]);
        let dealt: Vec<_> = copies.iter().map(|(o, c)| (*o, ranges(c, 2))).collect();
        let row = |src, dst| vec![[0, 2, 0, 2], [src, src, dst, dst]];
        assert_eq!(
            dealt,
            [
                (0, row(0, 0)),
                (1, row(1, 0)),
                (0, row(2, 1)),
                (1, row(3, 1)),
                (0, row(4, 2)),
            ]
        );

        // Runs of 2 and 1 over 5: rows 0 1 3 4 left, row 2 right.
        let (want1, want2, copies) = layout(1, 2, 1, &input, [&other, &other]);
        assert_eq!((want1.dim[1], want2.dim[1]), (4, 1));
        let dealt: Vec<_> = copies.iter().map(|(o, c)| (*o, ranges(c, 2)[1])).collect();
        assert_eq!(
            dealt,
            [(0, [0, 1, 0, 1]), (1, [2, 2, 0, 0]), (0, [3, 4, 2, 3])]
        );

        let (want1, want2, copies) = layout(1, 2, 0, &input, [&other, &other]);
        assert_eq!((want1.dim[1], want2.dim[1]), (2, 0));
        assert_eq!(copies.len(), 1);
        assert_eq!(ranges(&copies[0].1, 2)[1], [0, 1, 0, 1]);
        let (want1, want2, copies) = layout(1, 0, 0, &input, [&other, &other]);
        assert_eq!((want1.dim[1], want2.dim[1]), (0, 0));
        assert_eq!(copies.len(), 1);
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
    fn calc_resizes_clears_and_copies_each_run() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.demultiplex", &["@scan_b".into(), 2i64.into()]);
        assert!(!x.is_null(), "{:?}", k.console_log());
        assert_eq!(k.attr_get(x, "scan_a").unwrap(), [MockAtom::Long(1)]);
        assert_eq!(k.attr_get(x, "scan_b").unwrap(), [MockAtom::Long(2)]);
        assert_eq!(
            k.attr_get(x, "demultiplexdim").unwrap(),
            [MockAtom::Long(0)]
        );
        assert_eq!(k.attr_get(x, "autoclear").unwrap(), [MockAtom::Long(1)]);
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        // SAFETY: the registered `matrix_calc` has this signature.
        let calc: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
        ) -> max_sys::t_jit_err = unsafe { std::mem::transmute(f) };
        // Six columns: runs 0 | 1 2 | 3 | 4 5.
        let input = MatrixFixture::new(raw(MatrixDataType::Char, 1, &[6]));
        let run = |out1: &MatrixFixture, out2: &MatrixFixture| {
            let ins = MatrixList::new(&[&input]);
            let outs = MatrixList::new(&[out1, out2]);
            // SAFETY: the lists hold live matrices for the call.
            unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
        };
        let left = MatrixFixture::new(raw(MatrixDataType::Char, 1, &[2]));
        let right = MatrixFixture::new(raw(MatrixDataType::Char, 1, &[4]));
        k.take_console_log();
        assert_eq!(run(&left, &left), 0);
        let mut calls = vec!["setinfo", "setinfo", "clear", "clear"];
        calls.extend(["frommatrix"; 4]);
        assert_eq!(unsupported(&k), calls);
        k.attr_set(x, "autoclear", &[MockAtom::Long(0)]).unwrap();
        assert_eq!(run(&left, &right), 0);
        assert_eq!(unsupported(&k), ["frommatrix"; 4]);
        assert_eq!(input.lock_state(), 0);
        assert!(k.free_object(x));
    }
}
