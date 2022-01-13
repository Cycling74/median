//! Matrix iterators

use super::*;

//iterator types can be u8, f32, f64, t_atom_long
//iterators could pad with zeros if the requested size is bigger than the data size

fn assert_size<T>(info: &MatrixInfo) -> Result<(), ()> {
    //assert that we have the correct size
    let s = if info.is_char() {
        std::mem::size_of::<u8>()
    } else if info.is_f32() {
        std::mem::size_of::<f32>()
    } else if info.is_f64() {
        std::mem::size_of::<f64>()
    } else if info.is_long() {
        std::mem::size_of::<max_sys::t_atom_long>()
    } else {
        panic!("unsupported type");
    };
    if s == std::mem::size_of::<T>() {
        Ok(())
    } else {
        Err(())
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

impl<'a, T> Matrix2DEntryIter<'a, T> {
    pub unsafe fn new(inner: *mut c_char, info: &MatrixInfo) -> Result<Self, ()> {
        //assert that we have the correct size
        assert_size::<T>(info)?;

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

impl<'a, T> Iterator for Matrix2DEntryIter<'a, T> {
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

impl<'a, T> ExactSizeIterator for Matrix2DEntryIter<'a, T> {
    fn len(&self) -> usize {
        self.rows * self.cols
    }
}
