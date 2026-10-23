//! Borrowed Jitter matrices and checked, stride-aware views of their data.

use super::JitResult;
use core::ffi::{c_long, c_void};
use max_sys::{t_jit_error_code as err, t_jit_matrix_info};
use std::{marker::PhantomData, ptr::NonNull};

pub mod iter;
mod mop;
pub mod parallel;
pub use mop::{Count, IOCount, MOP};

pub const JIT_MATRIX_MAX_DIMCOUNT: usize = 32;
pub const JIT_MATRIX_MAX_PLANECOUNT: usize = 32;

/// Jitter's `long` cells are always 32 bits, including on 64-bit macOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixDataType {
    Char,
    Int32,
    Float32,
    Float64,
}

impl MatrixDataType {
    pub fn symbol(self) -> crate::symbol::SymbolRef {
        match self {
            Self::Char => crate::sym!("char"),
            Self::Int32 => crate::sym!("long"),
            Self::Float32 => crate::sym!("float32"),
            Self::Float64 => crate::sym!("float64"),
        }
    }

    pub const fn size(self) -> usize {
        match self {
            Self::Char => 1,
            Self::Int32 | Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }
}

/// A copy of matrix metadata. Views validate it against their actual backing storage.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct MatrixInfo(t_jit_matrix_info);

impl MatrixInfo {
    /// Describe a tightly packed matrix. Dimensions are ordered fastest first.
    pub fn new(kind: MatrixDataType, planes: usize, dims: &[usize]) -> JitResult<Self> {
        if !(1..=32).contains(&planes) || !(1..=32).contains(&dims.len()) {
            return Err(err::JIT_ERR_OUT_OF_BOUNDS);
        }
        let mut info = t_jit_matrix_info {
            size: 0,
            type_: kind.symbol().as_ptr(),
            flags: 0,
            dimcount: dims.len() as c_long,
            dim: [0; 32],
            dimstride: [0; 32],
            planecount: planes as c_long,
        };
        let mut stride = kind.size() * planes;
        for (i, &dim) in dims.iter().enumerate() {
            info.dim[i] = dim.try_into().map_err(|_| err::JIT_ERR_OUT_OF_BOUNDS)?;
            info.dimstride[i] = stride.try_into().map_err(|_| err::JIT_ERR_OUT_OF_BOUNDS)?;
            stride = stride.checked_mul(dim).ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?;
        }
        info.size = stride.try_into().map_err(|_| err::JIT_ERR_OUT_OF_BOUNDS)?;
        Ok(Self(info))
    }

    /// Copy metadata, including padding/strides. Does not dereference `type_` or trust the sizes.
    pub fn from_raw(info: t_jit_matrix_info) -> Self {
        Self(info)
    }
    pub fn as_raw(&self) -> &t_jit_matrix_info {
        &self.0
    }
    pub fn kind(&self) -> JitResult<MatrixDataType> {
        for kind in [
            MatrixDataType::Char,
            MatrixDataType::Int32,
            MatrixDataType::Float32,
            MatrixDataType::Float64,
        ] {
            if self.0.type_ == kind.symbol().as_ptr() {
                return Ok(kind);
            }
        }
        Err(err::JIT_ERR_MISMATCH_TYPE)
    }
    pub fn dimensions(&self) -> JitResult<&[c_long]> {
        let count = usize::try_from(self.0.dimcount).map_err(|_| err::JIT_ERR_MISMATCH_DIM)?;
        if !(1..=32).contains(&count) {
            return Err(err::JIT_ERR_MISMATCH_DIM);
        }
        Ok(&self.0.dim[..count])
    }
    pub fn planes(&self) -> JitResult<usize> {
        let planes = usize::try_from(self.0.planecount).map_err(|_| err::JIT_ERR_MISMATCH_PLANE)?;
        if !(1..=32).contains(&planes) {
            return Err(err::JIT_ERR_MISMATCH_PLANE);
        }
        Ok(planes)
    }
}

/// A non-owning matrix handle, confined to the call/lifetime that supplied it.
/// It is neither `Send` nor `Sync`.
pub struct Matrix<'a> {
    inner: NonNull<c_void>,
    _lifetime: PhantomData<&'a mut c_void>,
}

impl<'a> Matrix<'a> {
    /// # Safety
    /// `inner` must implement the Jitter matrix interface and remain alive for `'a`.
    /// Access and locking must obey the host's thread restrictions for that object.
    pub unsafe fn from_raw(inner: *mut c_void) -> JitResult<Self> {
        Ok(Self {
            inner: NonNull::new(inner).ok_or(err::JIT_ERR_INVALID_PTR)?,
            _lifetime: PhantomData,
        })
    }
    pub fn as_ptr(&self) -> *mut c_void {
        self.inner.as_ptr()
    }
    pub fn lock(&mut self) -> MatrixGuard<'_> {
        // SAFETY: the handle's constructor guarantees a live matrix interface.
        let previous = unsafe {
            max_sys::jit_object_method(self.as_ptr(), crate::sym!("lock").as_ptr(), 1 as c_long)
                as c_long
        };
        MatrixGuard {
            inner: self.inner,
            previous,
            _lifetime: PhantomData,
        }
    }
}

/// Restores the matrix's previous lock state, including during unwinding.
/// Jitter locking does not by itself authorize making aliased Rust references.
pub struct MatrixGuard<'a> {
    inner: NonNull<c_void>,
    previous: c_long,
    _lifetime: PhantomData<&'a mut c_void>,
}

impl MatrixGuard<'_> {
    pub fn info(&self) -> JitResult<MatrixInfo> {
        // Zero initialization avoids assuming initialized output if a nonstandard matrix refuses.
        let mut info: t_jit_matrix_info = unsafe { std::mem::zeroed() };
        // SAFETY: live locked matrix and writable, correctly sized metadata.
        let code = unsafe {
            max_sys::jit_object_method(
                self.inner.as_ptr(),
                crate::sym!("getinfo").as_ptr(),
                &mut info,
            ) as isize
        };
        if code != 0 {
            return Err(code as _);
        }
        let info = MatrixInfo::from_raw(info);
        info.kind()?;
        info.dimensions()?;
        info.planes()?;
        Ok(info)
    }

    /// Obtain a raw data view. Creating references requires `chunks`/`chunks_mut`'s contracts.
    pub fn data(&mut self) -> JitResult<MatrixData<'_>> {
        let info = self.info()?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        // SAFETY: live matrix and writable pointer slot. The returned pointer is not dereferenced.
        unsafe {
            max_sys::jit_object_method(
                self.inner.as_ptr(),
                crate::sym!("getdata").as_ptr(),
                &mut ptr,
            );
        }
        Ok(MatrixData {
            ptr: NonNull::new(ptr).ok_or(err::JIT_ERR_INVALID_PTR)?,
            info,
            _lifetime: PhantomData,
        })
    }
}

impl Drop for MatrixGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: lifetime is tied to the live matrix handle. Restore exactly the saved state.
        unsafe {
            max_sys::jit_object_method(
                self.inner.as_ptr(),
                crate::sym!("lock").as_ptr(),
                self.previous,
            );
        }
    }
}

/// Raw matrix storage borrowed from a lock guard. No ownership or exclusivity is implied.
pub struct MatrixData<'a> {
    ptr: NonNull<u8>,
    info: MatrixInfo,
    _lifetime: PhantomData<&'a mut [u8]>,
}

impl MatrixData<'_> {
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }
    pub fn info(&self) -> &MatrixInfo {
        &self.info
    }

    fn len<T: iter::JitEntryType>(&self) -> JitResult<usize> {
        let size = usize::try_from(self.info.0.size).map_err(|_| err::JIT_ERR_OUT_OF_BOUNDS)?;
        if self.info.0.size as u64 == 0xffff_ffff
            || size > isize::MAX as usize
            || size % std::mem::size_of::<T>() != 0
            || self.ptr.as_ptr() as usize % std::mem::align_of::<T>() != 0
        {
            return Err(err::JIT_ERR_OUT_OF_BOUNDS);
        }
        Ok(size / std::mem::size_of::<T>())
    }

    /// # Safety
    /// The matrix must provide `info.size` bytes of initialized storage, stable and free of
    /// writes for the returned borrow. This includes writes by other Max objects/threads.
    pub unsafe fn chunks<T: iter::JitEntryType>(
        &self,
    ) -> JitResult<iter::Matrix2DChunkIter<'_, T>> {
        let len = self.len::<T>()?;
        // SAFETY: caller guarantees the readable allocation; alignment and length checked above.
        let data = unsafe { std::slice::from_raw_parts(self.ptr.as_ptr().cast(), len) };
        iter::Matrix2DChunkIter::new(data, &self.info)
    }

    /// # Safety
    /// The matrix must provide `info.size` bytes of initialized storage exclusively accessible
    /// for the returned borrow. No other view, Max object or worker may access those bytes.
    pub unsafe fn chunks_mut<T: iter::JitEntryType>(
        &mut self,
    ) -> JitResult<iter::Matrix2DChunkIterMut<'_, T>> {
        let len = self.len::<T>()?;
        // SAFETY: caller guarantees exclusive storage; alignment and length checked above.
        let data = unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr().cast(), len) };
        iter::Matrix2DChunkIterMut::new(data, &self.info)
    }
}
