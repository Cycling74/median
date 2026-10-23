//! Checked matrix iterators. Construction takes a Rust slice, validates all dimensions and
//! strides, and rejects overlapping cells before any mutable reference can be returned.

use super::{MatrixDataType, MatrixInfo};
use crate::jit::JitResult;
use max_sys::t_jit_error_code as err;
use std::iter::FusedIterator;

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for i32 {}
    impl Sealed for f32 {}
    impl Sealed for f64 {}
}

/// The four Jitter cell types. Sealed because arbitrary Rust types need not admit every bit pattern.
pub trait JitEntryType: sealed::Sealed + Copy + Send + Sync + 'static {
    const KIND: MatrixDataType;
}
impl JitEntryType for u8 {
    const KIND: MatrixDataType = MatrixDataType::Char;
}
impl JitEntryType for i32 {
    const KIND: MatrixDataType = MatrixDataType::Int32;
}
impl JitEntryType for f32 {
    const KIND: MatrixDataType = MatrixDataType::Float32;
}
impl JitEntryType for f64 {
    const KIND: MatrixDataType = MatrixDataType::Float64;
}

#[derive(Clone, Copy)]
struct Layout {
    dims: [usize; 32],
    strides: [usize; 32],
    dimcount: usize,
    planes: usize,
    chunk_span: usize,
    chunks: usize,
    entries: usize,
}

impl Layout {
    fn new<T: JitEntryType>(info: &MatrixInfo, len: usize) -> JitResult<Self> {
        if info.kind()? != T::KIND {
            return Err(err::JIT_ERR_MISMATCH_TYPE);
        }
        let dimensions = info.dimensions()?;
        let planes = info.planes()?;
        let mut layout = Self {
            dims: [1; 32],
            strides: [0; 32],
            dimcount: dimensions.len(),
            planes,
            chunk_span: 0,
            chunks: 1,
            entries: 0,
        };
        for (i, &dim) in dimensions.iter().enumerate() {
            layout.dims[i] = dim.try_into().map_err(|_| err::JIT_ERR_MISMATCH_DIM)?;
        }
        // An empty matrix has no references to produce, irrespective of its unused strides.
        if layout.dims.contains(&0) {
            layout.chunks = 0;
            return Ok(layout);
        }
        let mut span = planes;
        for i in 0..layout.dimcount {
            let bytes: usize = info.as_raw().dimstride[i]
                .try_into()
                .map_err(|_| err::JIT_ERR_MISMATCH_DIM)?;
            if bytes % std::mem::size_of::<T>() != 0 {
                return Err(err::JIT_ERR_MISMATCH_DIM);
            }
            let stride = bytes / std::mem::size_of::<T>();
            // A singleton dimension does not step. Every other dimension must start after
            // the entire lower-dimensional span, including row padding.
            if layout.dims[i] > 1 && stride < span {
                return Err(err::JIT_ERR_MISMATCH_DIM);
            }
            layout.strides[i] = stride;
            span = stride
                .checked_mul(layout.dims[i] - 1)
                .and_then(|n| n.checked_add(span))
                .ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?;
            if i < 2 {
                layout.chunk_span = span;
            } else {
                layout.chunks = layout
                    .chunks
                    .checked_mul(layout.dims[i])
                    .ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?;
            }
        }
        if span > len {
            return Err(err::JIT_ERR_OUT_OF_BOUNDS);
        }
        layout.entries = layout.dims[0]
            .checked_mul(layout.dims[1])
            .ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?;
        Ok(layout)
    }

    fn chunk_offset(&self, mut index: usize) -> usize {
        let mut offset = 0;
        for i in 2..self.dimcount {
            offset += (index % self.dims[i]) * self.strides[i];
            index /= self.dims[i];
        }
        offset
    }
    fn entry_offset(&self, index: usize) -> usize {
        (index / self.dims[0]) * self.strides[1] + (index % self.dims[0]) * self.strides[0]
    }
}

pub struct Matrix2DChunk<'a, T> {
    data: &'a [T],
    layout: Layout,
}
pub struct Matrix2DChunkMut<'a, T> {
    data: &'a mut [T],
    layout: Layout,
}
pub struct Matrix2DChunkIter<'a, T> {
    data: &'a [T],
    layout: Layout,
    next: usize,
}
pub struct Matrix2DChunkIterMut<'a, T> {
    rest: &'a mut [T],
    layout: Layout,
    next: usize,
    consumed: usize,
}
pub struct Matrix2DEntryIter<'a, T> {
    data: &'a [T],
    layout: Layout,
    next: usize,
}
pub struct Matrix2DEntryIterMut<'a, T> {
    rest: &'a mut [T],
    layout: Layout,
    next: usize,
    consumed: usize,
}

impl<'a, T: JitEntryType> Matrix2DChunkIter<'a, T> {
    pub fn new(data: &'a [T], info: &MatrixInfo) -> JitResult<Self> {
        Ok(Self {
            layout: Layout::new::<T>(info, data.len())?,
            data,
            next: 0,
        })
    }
}
impl<'a, T: JitEntryType> Matrix2DChunkIterMut<'a, T> {
    pub fn new(data: &'a mut [T], info: &MatrixInfo) -> JitResult<Self> {
        Ok(Self {
            layout: Layout::new::<T>(info, data.len())?,
            rest: data,
            next: 0,
            consumed: 0,
        })
    }
}
impl<T> Matrix2DChunk<'_, T> {
    pub fn entry_iter(&self) -> Matrix2DEntryIter<'_, T> {
        Matrix2DEntryIter {
            data: self.data,
            layout: self.layout,
            next: 0,
        }
    }
}
impl<T> Matrix2DChunkMut<'_, T> {
    pub fn entry_iter(&self) -> Matrix2DEntryIter<'_, T> {
        Matrix2DEntryIter {
            data: self.data,
            layout: self.layout,
            next: 0,
        }
    }
    pub fn entry_iter_mut(&mut self) -> Matrix2DEntryIterMut<'_, T> {
        Matrix2DEntryIterMut {
            rest: self.data,
            layout: self.layout,
            next: 0,
            consumed: 0,
        }
    }
}

impl<'a, T> Iterator for Matrix2DChunkIter<'a, T> {
    type Item = Matrix2DChunk<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next == self.layout.chunks {
            return None;
        }
        let offset = self.layout.chunk_offset(self.next);
        self.next += 1;
        Some(Matrix2DChunk {
            data: &self.data[offset..offset + self.layout.chunk_span],
            layout: self.layout,
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.layout.chunks - self.next;
        (n, Some(n))
    }
}
impl<'a, T> Iterator for Matrix2DChunkIterMut<'a, T> {
    type Item = Matrix2DChunkMut<'a, T>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next == self.layout.chunks {
            return None;
        }
        let offset = self.layout.chunk_offset(self.next);
        let rest = std::mem::take(&mut self.rest);
        let (_, rest) = rest.split_at_mut(offset - self.consumed);
        let (data, rest) = rest.split_at_mut(self.layout.chunk_span);
        self.rest = rest;
        self.consumed = offset + self.layout.chunk_span;
        self.next += 1;
        Some(Matrix2DChunkMut {
            data,
            layout: self.layout,
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.layout.chunks - self.next;
        (n, Some(n))
    }
}
impl<'a, T> Iterator for Matrix2DEntryIter<'a, T> {
    type Item = &'a [T];
    fn next(&mut self) -> Option<Self::Item> {
        if self.next == self.layout.entries {
            return None;
        }
        let offset = self.layout.entry_offset(self.next);
        self.next += 1;
        Some(&self.data[offset..offset + self.layout.planes])
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.layout.entries - self.next;
        (n, Some(n))
    }
}
impl<'a, T> Iterator for Matrix2DEntryIterMut<'a, T> {
    type Item = &'a mut [T];
    fn next(&mut self) -> Option<Self::Item> {
        if self.next == self.layout.entries {
            return None;
        }
        let offset = self.layout.entry_offset(self.next);
        let rest = std::mem::take(&mut self.rest);
        let (_, rest) = rest.split_at_mut(offset - self.consumed);
        let (data, rest) = rest.split_at_mut(self.layout.planes);
        self.rest = rest;
        self.consumed = offset + self.layout.planes;
        self.next += 1;
        Some(data)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.layout.entries - self.next;
        (n, Some(n))
    }
}
macro_rules! exact_fused {
    ($($iter:ident),*) => { $(
        impl<T> ExactSizeIterator for $iter<'_, T> {}
        impl<T> FusedIterator for $iter<'_, T> {}
    )* };
}
exact_fused!(
    Matrix2DChunkIter,
    Matrix2DChunkIterMut,
    Matrix2DEntryIter,
    Matrix2DEntryIterMut
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_is_i32_and_lengths_count_remaining_cells() {
        let _k = median_mock::lock();
        let info = MatrixInfo::new(MatrixDataType::Int32, 2, &[3, 2]).unwrap();
        let data: Vec<i32> = (0..12).collect();
        let mut chunks = Matrix2DChunkIter::new(&data, &info).unwrap();
        assert_eq!(chunks.len(), 1);
        let chunk = chunks.next().unwrap();
        let mut entries = chunk.entry_iter();
        assert_eq!(entries.len(), 6);
        assert_eq!(entries.next(), Some(&[0, 1][..]));
        assert_eq!(entries.len(), 5);
        assert_eq!(entries.last(), Some(&[10, 11][..]));
        assert_eq!(chunks.len(), 0);
        assert!(chunks.next().is_none());
        assert!(Matrix2DChunkIter::new(&[0f32; 12], &info).is_err());
    }

    #[test]
    fn mutable_cells_and_chunks_are_disjoint_and_skip_padding() {
        let _k = median_mock::lock();
        let mut raw = *MatrixInfo::new(MatrixDataType::Int32, 2, &[2, 2, 2, 2])
            .unwrap()
            .as_raw();
        raw.dimstride[..4].copy_from_slice(&[12, 32, 80, 200]);
        let info = MatrixInfo::from_raw(raw);
        let mut data = vec![-1i32; 83];
        let mut chunks = Matrix2DChunkIterMut::new(&mut data, &info).unwrap();
        // Keep all chunks and every cell alive together; mutable iteration must never overlap.
        let mut all: Vec<_> = chunks.by_ref().collect();
        assert_eq!(chunks.len(), 0);
        assert_eq!(all.len(), 4);
        for (i, chunk) in all.iter_mut().enumerate() {
            let mut cells: Vec<_> = chunk.entry_iter_mut().collect();
            assert_eq!(cells.len(), 4);
            for (j, cell) in cells.iter_mut().enumerate() {
                cell.fill((i * 4 + j) as i32);
            }
        }
        drop(all);
        assert_eq!(&data[..5], &[0, 0, -1, 1, 1]);
        assert_eq!(data[8], 2);
        assert_eq!(data[20], 4);
        assert_eq!(data[50], 8);
        assert_eq!(data[81], 15);
    }

    #[test]
    fn rejects_bad_metadata_and_bounds_before_iteration() {
        let _k = median_mock::lock();
        let raw = *MatrixInfo::new(MatrixDataType::Float64, 2, &[3, 2])
            .unwrap()
            .as_raw();
        for change in 0..7 {
            let mut bad = raw;
            match change {
                0 => bad.dimcount = 33,
                1 => bad.dim[0] = -1,
                2 => bad.planecount = 0,
                3 => bad.dimstride[0] = 8,  // overlaps two-plane cells
                4 => bad.dimstride[1] = 16, // overlaps rows
                5 => bad.dimstride[0] = 17, // misaligned stride
                _ => bad.dim[1] = core::ffi::c_long::MAX,
            }
            assert!(
                Matrix2DChunkIterMut::new(&mut [0f64; 12], &MatrixInfo::from_raw(bad)).is_err()
            );
        }
        assert!(Matrix2DChunkIter::new(&[0f64; 11], &MatrixInfo::from_raw(raw)).is_err());
    }

    #[test]
    fn handles_one_dimension_empty_matrices_and_all_32_dimensions() {
        let _k = median_mock::lock();
        for dims in [vec![2], vec![1; 32], vec![2, 0, 3]] {
            let info = MatrixInfo::new(MatrixDataType::Char, 1, &dims).unwrap();
            let count = dims.iter().product::<usize>();
            let data = vec![7u8; count];
            let chunks = Matrix2DChunkIter::new(&data, &info).unwrap();
            let actual: usize = chunks.map(|c| c.entry_iter().count()).sum();
            assert_eq!(actual, count);
        }
    }
}
