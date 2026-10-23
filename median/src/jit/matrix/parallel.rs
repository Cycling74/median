//! Low-level Jitter parallel dispatch. Worker views expose raw pointers, never an implicit
//! exclusive borrow: full-matrix flags and input/output aliasing must be considered by callers.

use super::MatrixInfo;
use crate::{jit::JitResult, method::MaxMethod};
use core::ffi::{c_char, c_long, c_void};
use max_sys::{t_jit_error_code as err, t_jit_matrix_info};
use std::{
    marker::PhantomData,
    sync::atomic::{AtomicU32, Ordering},
};

/// Metadata and the start of the worker's region. Strides are those of the original matrix;
/// the callback's dimension slice describes this worker's extent.
pub struct ParallelMatrix<'a> {
    info: MatrixInfo,
    data: *mut u8,
    _lifetime: PhantomData<&'a [u8]>,
}
impl ParallelMatrix<'_> {
    pub fn info(&self) -> &MatrixInfo {
        &self.info
    }
    pub fn as_ptr(&self) -> *mut u8 {
        self.data
    }
}

struct Holder<F> {
    func: F,
    error: AtomicU32,
}
impl<F> Holder<F> {
    fn result(&self) -> JitResult<()> {
        match self.error.load(Ordering::Relaxed) {
            0 => Ok(()),
            code => Err(code),
        }
    }
    // Called only from our two C trampolines. The host must supply readable metadata and
    // exactly dimcount readable dimension entries; no 32-entry array is assumed here.
    unsafe fn invoke<const N: usize>(
        &self,
        count: c_long,
        dim: *const c_long,
        planes: c_long,
        infos: [*const t_jit_matrix_info; N],
        data: [*mut c_char; N],
    ) where
        F: Fn(&[c_long], usize, &[ParallelMatrix<'_>; N]) -> JitResult<()>,
    {
        let result = crate::ffi::guard_global(
            "Jitter parallel callback",
            Err(err::JIT_ERR_GENERIC),
            || {
                if !(1..=32).contains(&count)
                    || !(1..=32).contains(&planes)
                    || dim.is_null()
                    || infos.iter().any(|p| p.is_null())
                    || data.iter().any(|p| p.is_null())
                {
                    return Err(err::JIT_ERR_INVALID_PTR);
                }
                // SAFETY: callback contract, bounded count; only count entries are read.
                let dims = unsafe { std::slice::from_raw_parts(dim, count as usize) };
                if dims.iter().any(|&n| n < 0) {
                    return Err(err::JIT_ERR_MISMATCH_DIM);
                }
                let matrices = std::array::from_fn(|i| ParallelMatrix {
                    // SAFETY: host supplies one initialized metadata record for each matrix.
                    info: MatrixInfo::from_raw(unsafe { *infos[i] }),
                    data: data[i].cast(),
                    _lifetime: PhantomData,
                });
                (self.func)(dims, planes as usize, &matrices)
            },
        );
        if let Err(code) = result {
            // Preserve the first error. Workers may finish in any order.
            let _ = self
                .error
                .compare_exchange(0, code, Ordering::Relaxed, Ordering::Relaxed);
        }
    }
}

impl<F> Holder<F>
where
    F: Fn(&[c_long], usize, &[ParallelMatrix<'_>; 2]) -> JitResult<()>,
{
    unsafe extern "C" fn compute2(
        holder: *const Self,
        count: c_long,
        dim: *const c_long,
        planes: c_long,
        i0: *const t_jit_matrix_info,
        p0: *mut c_char,
        i1: *const t_jit_matrix_info,
        p1: *mut c_char,
    ) {
        // SAFETY: calc2 keeps the holder alive until the synchronous dispatch joins its workers.
        unsafe {
            (*holder).invoke(count, dim, planes, [i0, i1], [p0, p1]);
        }
    }
}
impl<F> Holder<F>
where
    F: Fn(&[c_long], usize, &[ParallelMatrix<'_>; 3]) -> JitResult<()>,
{
    unsafe extern "C" fn compute3(
        holder: *const Self,
        count: c_long,
        dim: *const c_long,
        planes: c_long,
        i0: *const t_jit_matrix_info,
        p0: *mut c_char,
        i1: *const t_jit_matrix_info,
        p1: *mut c_char,
        i2: *const t_jit_matrix_info,
        p2: *mut c_char,
    ) {
        // SAFETY: calc3 keeps the holder alive until the synchronous dispatch joins its workers.
        unsafe {
            (*holder).invoke(count, dim, planes, [i0, i1, i2], [p0, p1, p2]);
        }
    }
}

fn validate(dim: &[c_long], planes: usize) -> JitResult<()> {
    if !(1..=32).contains(&dim.len()) || !(1..=32).contains(&planes) || dim.iter().any(|&n| n < 0) {
        return Err(err::JIT_ERR_MISMATCH_DIM);
    }
    Ok(())
}

/// Dispatch two matrices, returning the first worker error or a contained panic.
///
/// # Safety
/// Matrices must remain locked, alive and match their metadata for the whole synchronous call.
/// Dimensions/planes must fit every matrix. Each data pointer must cover the described extent.
/// The callback must obey shared/exclusive access across workers and matrices, particularly
/// when full-matrix flags are used. No callback may retain any pointer after this call.
pub unsafe fn calc2<F>(
    dim: &[c_long],
    planes: usize,
    matrices: [(&MatrixInfo, *mut u8); 2],
    flags: [c_long; 2],
    func: F,
) -> JitResult<()>
where
    F: Send + Sync + Fn(&[c_long], usize, &[ParallelMatrix<'_>; 2]) -> JitResult<()>,
{
    validate(dim, planes)?;
    let holder = Holder {
        func,
        error: AtomicU32::new(0),
    };
    let mut dims = [0; 32];
    dims[..dim.len()].copy_from_slice(dim);
    let mut infos = [*matrices[0].0.as_raw(), *matrices[1].0.as_raw()];
    // SAFETY: caller supplies valid storage and synchronization; all call-local records live
    // until Jitter returns after joining workers. The erased method is restored by Jitter.
    unsafe {
        max_sys::jit_parallel_ndim_simplecalc2(
            Some(std::mem::transmute::<
                unsafe extern "C" fn(
                    *const Holder<F>,
                    c_long,
                    *const c_long,
                    c_long,
                    *const t_jit_matrix_info,
                    *mut c_char,
                    *const t_jit_matrix_info,
                    *mut c_char,
                ),
                MaxMethod,
            >(Holder::<F>::compute2)),
            &holder as *const _ as *mut c_void,
            dim.len() as c_long,
            dims.as_mut_ptr(),
            planes as c_long,
            &mut infos[0],
            matrices[0].1.cast(),
            &mut infos[1],
            matrices[1].1.cast(),
            flags[0],
            flags[1],
        );
    }
    holder.result()
}

/// Three-matrix counterpart of [`calc2`]; every worker receives all three matrices.
///
/// # Safety
/// The storage, dimension, lifetime and concurrency contracts of [`calc2`] apply to all three.
pub unsafe fn calc3<F>(
    dim: &[c_long],
    planes: usize,
    matrices: [(&MatrixInfo, *mut u8); 3],
    flags: [c_long; 3],
    func: F,
) -> JitResult<()>
where
    F: Send + Sync + Fn(&[c_long], usize, &[ParallelMatrix<'_>; 3]) -> JitResult<()>,
{
    validate(dim, planes)?;
    let holder = Holder {
        func,
        error: AtomicU32::new(0),
    };
    let mut dims = [0; 32];
    dims[..dim.len()].copy_from_slice(dim);
    let mut infos = [
        *matrices[0].0.as_raw(),
        *matrices[1].0.as_raw(),
        *matrices[2].0.as_raw(),
    ];
    // SAFETY: same synchronous, caller-provided storage contract as calc2.
    unsafe {
        max_sys::jit_parallel_ndim_simplecalc3(
            Some(std::mem::transmute::<
                unsafe extern "C" fn(
                    *const Holder<F>,
                    c_long,
                    *const c_long,
                    c_long,
                    *const t_jit_matrix_info,
                    *mut c_char,
                    *const t_jit_matrix_info,
                    *mut c_char,
                    *const t_jit_matrix_info,
                    *mut c_char,
                ),
                MaxMethod,
            >(Holder::<F>::compute3)),
            &holder as *const _ as *mut c_void,
            dim.len() as c_long,
            dims.as_mut_ptr(),
            planes as c_long,
            &mut infos[0],
            matrices[0].1.cast(),
            &mut infos[1],
            matrices[1].1.cast(),
            &mut infos[2],
            matrices[2].1.cast(),
            flags[0],
            flags[1],
            flags[2],
        );
    }
    holder.result()
}

/// Compute the shared extent from each matrix's own metadata (not the first matrix twice).
pub fn intersection(a: &MatrixInfo, b: &MatrixInfo) -> JitResult<(Vec<c_long>, usize)> {
    let dims: Vec<_> = a
        .dimensions()?
        .iter()
        .zip(b.dimensions()?)
        .map(|(&a, &b)| a.min(b))
        .collect();
    let planes = a.planes()?.min(b.planes()?);
    validate(&dims, planes)?;
    Ok((dims, planes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::matrix::MatrixDataType;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn workers_receive_only_dimcount_entries_and_distinct_matrix_info() {
        let _k = median_mock::lock();
        let a = MatrixInfo::new(MatrixDataType::Char, 1, &[8, 4]).unwrap();
        let b = MatrixInfo::new(MatrixDataType::Float32, 2, &[3, 9]).unwrap();
        let c = MatrixInfo::new(MatrixDataType::Int32, 3, &[2, 2]).unwrap();
        assert_eq!(intersection(&a, &b), Ok((vec![3, 4], 1)));
        let seen = AtomicUsize::new(0);
        let holder = Holder {
            error: AtomicU32::new(0),
            func: |dims: &[c_long], planes, matrices: &[ParallelMatrix<'_>; 3]| {
                assert_eq!(dims, &[2, 2]);
                assert_eq!(planes, 1);
                assert_eq!(matrices[0].info().kind(), Ok(MatrixDataType::Char));
                assert_eq!(matrices[1].info().kind(), Ok(MatrixDataType::Float32));
                assert_eq!(matrices[2].info().kind(), Ok(MatrixDataType::Int32));
                seen.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        };
        // The callback reads no data. Each metadata pointer is live and dims has only 2 entries.
        let mut data = [0u8; 1];
        unsafe {
            Holder::compute3(
                &holder,
                2,
                [2, 2].as_ptr(),
                1,
                a.as_raw(),
                data.as_mut_ptr().cast(),
                b.as_raw(),
                data.as_mut_ptr().cast(),
                c.as_raw(),
                data.as_mut_ptr().cast(),
            );
        }
        assert_eq!(seen.load(Ordering::Relaxed), 1);
        assert_eq!(holder.result(), Ok(()));
    }

    #[test]
    fn worker_panics_and_invalid_dimensions_return_errors() {
        let k = median_mock::lock();
        let info = MatrixInfo::new(MatrixDataType::Char, 1, &[1]).unwrap();
        let holder = Holder {
            error: AtomicU32::new(0),
            func: |_: &[c_long], _, _: &[ParallelMatrix<'_>; 2]| -> JitResult<()> {
                panic!("worker failed")
            },
        };
        let mut data = [0u8; 1];
        // SAFETY: live records, one dimension, no data dereferences in the callback.
        unsafe {
            Holder::compute2(
                &holder,
                1,
                [1].as_ptr(),
                1,
                info.as_raw(),
                data.as_mut_ptr().cast(),
                info.as_raw(),
                data.as_mut_ptr().cast(),
            );
        }
        assert_eq!(holder.result(), Err(err::JIT_ERR_GENERIC));
        assert!(
            k.take_console_log()
                .iter()
                .any(|e| e.text.contains("worker failed"))
        );
        assert!(validate(&[], 1).is_err());
        assert!(validate(&[1; 33], 1).is_err());
        assert!(validate(&[-1], 1).is_err());
    }
}
