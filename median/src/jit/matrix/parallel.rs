use super::iter::Matrix2DChunkIter;
use super::*;
use crate::jit::JitResult;

pub type MatrixDataInfo<'a> = (&'a MatrixInfo, &'a MatrixData<'a>);

struct ParallelCalcHolder<F, const N: usize>
where
    F: Send + Sync + Fn(usize, &[c_long; JIT_MATRIX_MAX_DIMCOUNT], usize, &[MatrixDataInfo<'_>; N]),
{
    func: F,
}

impl<F> ParallelCalcHolder<F, 2>
where
    F: Send + Sync + Fn(usize, &[c_long; JIT_MATRIX_MAX_DIMCOUNT], usize, &[MatrixDataInfo<'_>; 2]),
{
    extern "C" fn compute_matrix(
        &self,
        dimcount: c_long,
        dim: *const c_long,
        planecount: c_long,
        mi0: &MatrixInfo,
        m0: *mut c_char,
        mi1: &MatrixInfo,
        m1: *mut c_char,
    ) {
        let (m0, m1, dim) = unsafe {
            (
                MatrixData::new(m0),
                MatrixData::new(m1),
                std::mem::transmute(dim),
            )
        };
        (self.func)(
            dimcount as _,
            dim,
            planecount as _,
            &[(mi0, &m0), (mi1, &m1)],
        );
    }
}

impl<F> ParallelCalcHolder<F, 3>
where
    F: Send + Sync + Fn(usize, &[c_long; JIT_MATRIX_MAX_DIMCOUNT], usize, &[MatrixDataInfo<'_>; 3]),
{
    extern "C" fn compute_matrix(
        &self,
        _dimcount: c_long,
        _dim: *const c_long,
        _planecount: c_long,
        _mi0: &MatrixInfo,
        _m0: *mut c_char,
        _mi1: &MatrixInfo,
        _m1: *mut c_char,
        _mi2: &MatrixInfo,
        _m2: *mut c_char,
    ) {
        //TODO
    }
}

pub fn calc2<'a, F>(
    dim_count: usize,
    dim: &[c_long; JIT_MATRIX_MAX_DIMCOUNT],
    plane_count: usize,
    matrix: &[MatrixDataInfo<'a>; 2],
    flags: &[c_long; 2],
    func: F,
) where
    F: Send + Sync + Fn(usize, &[c_long; JIT_MATRIX_MAX_DIMCOUNT], usize, &[MatrixDataInfo<'_>; 2]),
{
    let holder: ParallelCalcHolder<F, 2> = ParallelCalcHolder { func };
    unsafe {
        max_sys::jit_parallel_ndim_simplecalc2(
            Some(std::mem::transmute::<
                extern "C" fn(
                    &ParallelCalcHolder<F, 2>,
                    c_long,
                    *const c_long,
                    c_long,
                    &MatrixInfo,
                    *mut c_char,
                    &MatrixInfo,
                    *mut c_char,
                ),
                MaxMethod,
            >(ParallelCalcHolder::<F, 2>::compute_matrix)),
            &holder as *const ParallelCalcHolder<F, 2> as _,
            dim_count as _,
            dim.as_ptr() as _,
            plane_count as _,
            std::mem::transmute(matrix[0].0),
            matrix[0].1.inner() as _,
            std::mem::transmute(matrix[1].0),
            matrix[1].1.inner() as _,
            flags[0],
            flags[1],
        );
    }
}

pub fn calc2_intersection<'a, F>(matrix: &[MatrixDataInfo<'a>; 2], flags: &[c_long; 2], func: F)
where
    F: Send + Sync + Fn(usize, &[c_long; JIT_MATRIX_MAX_DIMCOUNT], usize, &[MatrixDataInfo<'_>; 2]),
{
    let dim_count = std::cmp::min(matrix[0].0.dim_count(), matrix[1].0.dim_count());
    let plane_count = std::cmp::min(matrix[0].0.plane_count(), matrix[1].0.plane_count());
    let mut dim: [c_long; JIT_MATRIX_MAX_DIMCOUNT] = [0; JIT_MATRIX_MAX_DIMCOUNT];
    for (d, i, o, _) in itertools::multizip((
        &mut dim,
        matrix[0].0.dim_sizes(),
        matrix[1].0.dim_sizes(),
        (0..dim_count),
    )) {
        *d = std::cmp::min(*i, *o);
    }
    calc2(dim_count, &dim, plane_count, matrix, flags, func);
}

pub fn calc2_intersection2d<'a, F, T0, T1, const P0: usize, const P1: usize>(
    m0: &mut MatrixGuard<'a>,
    m1: &mut MatrixGuard<'a>,
    flags: &[c_long; 2],
    func: F,
) -> JitResult<()>
where
    F: Send + Sync + Fn(Matrix2DChunkIter<'_, T0>, Matrix2DChunkIter<'_, T1>),
    T0: iter::JitEntryType,
    T1: iter::JitEntryType,
{
    let m0i = m0.info();
    let m1i = m0.info();

    iter::assert_type::<T0>(&m0i)?;
    iter::assert_type::<T1>(&m1i)?;
    if P0 > 0 && m0i.plane_count() != P0 {
        return Err(max_sys::t_jit_error_code::JIT_ERR_MISMATCH_PLANE);
    }
    if P1 > 0 && m1i.plane_count() != P1 {
        return Err(max_sys::t_jit_error_code::JIT_ERR_MISMATCH_PLANE);
    }

    let m0d = m0
        .data()
        .ok_or(max_sys::t_jit_error_code::JIT_ERR_INVALID_INPUT)?;
    let m1d = m1
        .data()
        .ok_or(max_sys::t_jit_error_code::JIT_ERR_INVALID_INPUT)?;

    let matrix = [(&m0i, &m0d), (&m1i, &m1d)];

    calc2_intersection(&matrix, flags, |dimcount, dims, planes, matrices| {
        let m0: Matrix2DChunkIter<'_, T0> = unsafe {
            Matrix2DChunkIter::new(
                dimcount as _,
                dims,
                planes as _,
                matrices[0].0,
                matrices[0].1.inner(),
            )
            .unwrap()
        };
        let m1: Matrix2DChunkIter<'_, T1> = unsafe {
            Matrix2DChunkIter::new(
                dimcount as _,
                dims,
                planes as _,
                matrices[1].0,
                matrices[1].1.inner(),
            )
            .unwrap()
        };
        func(m0, m1);
    });
    Ok(())
}

pub fn calc3<'a, F>(
    dim_count: usize,
    dim: &[c_long; JIT_MATRIX_MAX_DIMCOUNT],
    plane_count: usize,
    matrix: &[MatrixDataInfo<'a>; 3],
    flags: &[c_long; 3],
    func: F,
) where
    F: Send + Sync + Fn(usize, &[c_long; JIT_MATRIX_MAX_DIMCOUNT], usize, &[MatrixDataInfo<'_>; 3]),
{
    let holder: ParallelCalcHolder<F, 3> = ParallelCalcHolder { func };
    unsafe {
        max_sys::jit_parallel_ndim_simplecalc3(
            Some(std::mem::transmute::<
                extern "C" fn(
                    &ParallelCalcHolder<F, 3>,
                    c_long,
                    *const c_long,
                    c_long,
                    &MatrixInfo,
                    *mut c_char,
                    &MatrixInfo,
                    *mut c_char,
                    &MatrixInfo,
                    *mut c_char,
                ),
                MaxMethod,
            >(ParallelCalcHolder::<F, 3>::compute_matrix)),
            &holder as *const ParallelCalcHolder<F, 3> as _,
            dim_count as _,
            dim.as_ptr() as _,
            plane_count as _,
            std::mem::transmute(matrix[0].0),
            matrix[0].1.inner() as _,
            std::mem::transmute(matrix[1].0),
            matrix[1].1.inner() as _,
            std::mem::transmute(matrix[2].0),
            matrix[2].1.inner() as _,
            flags[0],
            flags[1],
            flags[2],
        );
    }
}
