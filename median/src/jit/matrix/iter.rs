//! Matrix iterators

use super::*;
use crate::jit::JitResult;

use std::any::TypeId;

const MATRIX_2D_REDUCE_DIM_COUNT: usize = JIT_MATRIX_MAX_DIMCOUNT as usize - 2;

//iterator types can be u8, f32, f64, t_atom_long
//iterators could pad with zeros if the requested size is bigger than the data size

/// Marker type for jitter matrix entries
pub trait JitEntryType: 'static {}

impl JitEntryType for u8 {}
impl JitEntryType for f32 {}
impl JitEntryType for f64 {}
impl JitEntryType for max_sys::t_atom_long {}

pub fn assert_type<T: JitEntryType>(info: &MatrixInfo) -> JitResult<()> {
    //assert that we have the correct size
    let id = if info.is_char() {
        TypeId::of::<u8>()
    } else if info.is_f32() {
        TypeId::of::<f32>()
    } else if info.is_f64() {
        TypeId::of::<f64>()
    } else if info.is_long() {
        TypeId::of::<max_sys::t_atom_long>()
    } else {
        //shouldn't ever happen because of JitEntryType
        panic!("unsupported type");
    };
    if id == TypeId::of::<T>() {
        Ok(())
    } else {
        Err(max_sys::t_jit_error_code::JIT_ERR_MISMATCH_TYPE)
    }
}

/// Iterate over each entry in a 2D matrix
pub struct Matrix2DEntryIter<'a, T> {
    _inner: *mut c_char,

    //pointers to current row/column, allows for addition of offsets/stride instead of
    //multiplication
    row: *mut c_char,
    col: *mut T,

    rows: usize,
    cols: usize,

    //row, column index
    indexr: usize,
    indexc: usize,

    plane_count: usize,
    rows_stride: isize,
    _phantom: PhantomData<&'a T>,
}

struct ChunkIterData {
    ptr: *mut c_char,
    stride: isize,
    len: usize,
    index: usize,
}

/// A 2D chunk of a matrix
pub struct Matrix2DChunk<'a, T> {
    inner: *mut c_char,
    info: MatrixInfo,
    _phantom: PhantomData<&'a T>,
}

/// Iterate over a Matrix in 2D chunks
pub struct Matrix2DChunkIter<'a, T> {
    _inner: *mut c_char,
    info: MatrixInfo,
    dims: [ChunkIterData; MATRIX_2D_REDUCE_DIM_COUNT],
    count: usize, //how many dimensions total (not including 2d)
    _phantom: PhantomData<&'a T>,
}

impl<'a, T> Matrix2DEntryIter<'a, T>
where
    T: JitEntryType,
{
    pub unsafe fn new(inner: *mut c_char, info: &MatrixInfo) -> JitResult<Self> {
        //assert that we have the correct type
        assert_type::<T>(info)?;

        assert!(!inner.is_null());
        assert!(info.dim_count() > 0);
        let sizes = info.dim_sizes();
        let strides = info.dim_strides();
        let plane_count = info.plane_count();
        let (cols, rows) = (sizes[0], sizes[1]);
        let rows_stride = strides[1] as isize;
        assert!(cols > 0);
        assert!(rows > 0);
        assert!(rows_stride > 0);
        Ok(Self {
            _inner: inner,
            row: inner,
            col: inner as _,

            rows: rows as _,
            cols: cols as _,

            indexr: 0,
            indexc: 0,

            plane_count,
            rows_stride,
            _phantom: Default::default(),
        })
    }
}

impl<'a, T> Iterator for Matrix2DEntryIter<'a, T>
where
    T: JitEntryType,
{
    type Item = &'a mut [T];
    fn next(&mut self) -> Option<Self::Item> {
        if self.indexr < self.rows {
            //get
            let v = Some(unsafe { std::slice::from_raw_parts_mut(self.col, self.plane_count) });
            //increment
            self.indexc += 1;
            if self.indexc < self.cols {
                unsafe {
                    self.col = self.col.offset(self.plane_count as _);
                }
            } else {
                self.indexr += 1;
                self.indexc = 0;
                if self.indexr < self.rows {
                    unsafe {
                        self.row = self.row.offset(self.rows_stride);
                        self.col = self.row as _;
                    }
                }
            }
            v
        } else {
            None
        }
    }
}

impl<'a, T> ExactSizeIterator for Matrix2DEntryIter<'a, T>
where
    T: JitEntryType,
{
    fn len(&self) -> usize {
        self.rows * self.cols
    }
}

impl ChunkIterData {
    fn done(&self) -> bool {
        self.index >= self.len
    }
    fn reset(&mut self, p: *mut c_char) {
        self.ptr = p;
        self.index = 0;
    }
    //incr and indicate overflow
    fn incr(&mut self) -> bool {
        self.index += 1;
        unsafe {
            self.ptr = self.ptr.offset(self.stride);
        }
        self.index >= self.len
    }
    fn ptr(&self) -> *mut c_char {
        self.ptr
    }
}

impl Default for ChunkIterData {
    fn default() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            index: 0,
            len: 0,
            stride: 0,
        }
    }
}

impl<'a, T> Matrix2DChunk<'a, T>
where
    T: JitEntryType,
{
    pub unsafe fn new(inner: *mut c_char, info: MatrixInfo) -> JitResult<Self> {
        assert_type::<T>(&info)?;

        //XXX assert details
        Ok(Self {
            inner,
            info,
            _phantom: Default::default(),
        })
    }

    /// Get an interator to each entry in the Matrix
    pub fn entry_iter(&self) -> Matrix2DEntryIter<'_, T> {
        unsafe { Matrix2DEntryIter::new(self.inner, &self.info).unwrap() }
    }
}

impl<'a, T> Matrix2DChunkIter<'a, T>
where
    T: JitEntryType,
{
    pub unsafe fn new(
        dimcount: usize,
        dim: &[c_long; JIT_MATRIX_MAX_DIMCOUNT],
        planecount: usize,
        info: &MatrixInfo,
        inner: *mut c_char,
    ) -> JitResult<Self> {
        assert!(dimcount > 0 && dimcount < JIT_MATRIX_MAX_DIMCOUNT);
        assert!(planecount > 0);
        assert_ne!(inner, std::ptr::null_mut());
        assert_type::<T>(info)?;

        //update some items here
        let mut info = info.clone();
        info.set_plane_count(planecount);

        //clamp to thet smallest dimension
        for (o, i) in info.dim_sizes_mut().iter_mut().zip(dim.iter()) {
            *o = std::cmp::min(*i, *o);
        }

        let mut dims: [ChunkIterData; MATRIX_2D_REDUCE_DIM_COUNT] = Default::default();
        let mut count = 1;
        //if there are fewer than 3 dimensions, add a single 3d entry so we have a length of 1
        if dimcount < 3 {
            let mut first = &mut dims[0];
            first.ptr = inner;
            first.index = 0;
            first.len = 1;
            first.stride = 0; //doesn't matter

            //1d special case.. needed?
            if dimcount < 2 {
                info.dim_sizes_mut()[1] = 1;
            }
        } else {
            count = dimcount - 2;
            let indim = dim;
            for (o, d, s) in itertools::multizip((
                dims.iter_mut(),
                indim[2..dimcount].iter(),
                info.dim_strides()[2..dimcount].iter(),
            )) {
                assert!(*d > 0);
                assert!(*s > 0);
                o.index = 0;
                o.ptr = inner;
                o.len = *d as _;
                o.stride = *s as isize;
            }
        }

        Ok(Self {
            _inner: inner,
            info,
            dims,
            count,
            _phantom: Default::default(),
        })
    }
}

impl<'a, T> Iterator for Matrix2DChunkIter<'a, T>
where
    T: JitEntryType,
{
    type Item = Matrix2DChunk<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.dims[self.count - 1].done() {
            None
        } else {
            //get
            let first = &self.dims[0];
            let cur: Self::Item =
                unsafe { Matrix2DChunk::new(first.ptr, self.info.clone()).expect("type match") };
            //iterate dimensions.. if there is an overflow, check the next, if it doesn't then
            //reset all the previous with the next's ptr
            if self.dims[0].incr() {
                let mut reset: Option<(*mut c_char, usize)> = None;
                for (i, d) in self.dims[1..self.count].iter_mut().enumerate() {
                    if !d.incr() {
                        //we skip the first entry and we don't want to reset this entry again
                        reset = Some((d.ptr(), i + 1));
                        break;
                    }
                }
                if let Some((reset, end)) = reset {
                    for d in self.dims[0..end].iter_mut() {
                        d.reset(reset)
                    }
                } else {
                    //TODO assert last is done?
                }
            }

            Some(cur)
        }
    }
}

/* TODO
impl<'a, T> ExactSizeIterator for Matrix2DChunkIter<'a, T>
where
    T: JitEntryType,
{
    fn len(&self) -> usize {
    }
}
*/
