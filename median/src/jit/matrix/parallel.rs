use super::*;

pub type MatrixDataInfo<'a> = (&'a MatrixInfo, &'a MatrixData<'a>);

struct ParallelCalcHolder<F, const N: usize>
where
    F: Send + Sync + Fn(usize, &[c_long], usize, &[MatrixDataInfo<'_>; N]),
{
    func: F,
}

impl<F> ParallelCalcHolder<F, 2>
where
    F: Send + Sync + Fn(usize, &[c_long], usize, &[MatrixDataInfo<'_>; 2]),
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
                std::slice::from_raw_parts(dim, JIT_MATRIX_MAX_DIMCOUNT),
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
    F: Send + Sync + Fn(usize, &[c_long], usize, &[MatrixDataInfo<'_>; 3]),
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
    F: Send + Sync + Fn(usize, &[c_long], usize, &[MatrixDataInfo<'_>; 2]),
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

pub fn calc3<'a, F>(
    dim_count: usize,
    dim: &[c_long; JIT_MATRIX_MAX_DIMCOUNT],
    plane_count: usize,
    matrix: &[MatrixDataInfo<'a>; 3],
    flags: &[c_long; 3],
    func: F,
) where
    F: Send + Sync + Fn(usize, &[c_long], usize, &[MatrixDataInfo<'_>; 3]),
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
