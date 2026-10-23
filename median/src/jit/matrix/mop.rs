//! Matrix-operator class adornments. A MOP belongs to its Jitter class after registration.

use super::{JitResult, MatrixDataType};
use core::ffi::{c_long, c_void};
use max_sys::t_jit_error_code as err;
use std::{marker::PhantomData, ptr::NonNull};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Variable,
    Fixed(usize),
}
impl Count {
    pub(crate) fn raw(self) -> JitResult<c_long> {
        match self {
            Self::Variable => Ok(-1),
            Self::Fixed(n) if n <= super::JIT_MATRIX_MAX_PLANECOUNT => Ok(n as c_long),
            _ => Err(err::JIT_ERR_OUT_OF_BOUNDS),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IOCount {
    pub inputs: Count,
    pub outputs: Count,
}

/// A borrowed MOP, used to configure a class before it is registered.
pub struct MOP<'a> {
    ptr: NonNull<c_void>,
    _owner: PhantomData<&'a mut c_void>,
}
impl<'a> MOP<'a> {
    /// # Safety
    /// `ptr` must be a live `jit_mop` adornment, exclusively configurable for `'a` on a thread
    /// where class registration is permitted. Its owner must keep it alive for this borrow.
    pub unsafe fn from_raw(ptr: *mut c_void) -> JitResult<Self> {
        Ok(Self {
            ptr: NonNull::new(ptr).ok_or(err::JIT_ERR_INVALID_PTR)?,
            _owner: PhantomData,
        })
    }
    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
    pub fn single_type(&mut self, kind: MatrixDataType) -> JitResult<()> {
        result(unsafe { max_sys::jit_mop_single_type(self.as_ptr(), kind.symbol().as_ptr()) })
    }
    pub fn single_plane_count(&mut self, planes: usize) -> JitResult<()> {
        if !(1..=super::JIT_MATRIX_MAX_PLANECOUNT).contains(&planes) {
            return Err(err::JIT_ERR_OUT_OF_BOUNDS);
        }
        result(unsafe { max_sys::jit_mop_single_planecount(self.as_ptr(), planes as c_long) })
    }
}

fn result(code: max_sys::t_jit_err) -> JitResult<()> {
    if code == 0 { Ok(()) } else { Err(code as _) }
}
