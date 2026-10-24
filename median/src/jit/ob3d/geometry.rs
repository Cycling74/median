use super::GLDataBuffer;
use crate::jit::{DrawContext, JitResult};
use core::ffi::{c_long, c_void};
use max_sys::{self as sys, t_jit_error_code as err};
use std::ptr::NonNull;

/// An owned geometry confined to a host GL callback. There is no safe constructor: the SDK
/// headers do not specify the geometry class's constructor or `setdata` retention contract.
pub struct GLGeometry<'ctx> {
    ptr: NonNull<c_void>,
    context: &'ctx DrawContext,
}
impl<'ctx> GLGeometry<'ctx> {
    /// # Safety
    /// Transfer one exclusively owned live Jitter geometry, releasable with `jit_object_free`
    /// in `context`. It must belong to the callback's render context and all resources it
    /// references must stay live until it is dropped. A borrowed geometry must not be adopted.
    pub unsafe fn from_raw(context: &'ctx DrawContext, ptr: *mut c_void) -> JitResult<Self> {
        Ok(Self {
            ptr: NonNull::new(ptr).ok_or(err::JIT_ERR_INVALID_PTR)?,
            context,
        })
    }
    pub fn as_ptr(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
    /// Upload one descriptor using a copying `setdata` implementation.
    ///
    /// # Safety
    /// Verify this geometry's `setdata` ABI is `(object, long, t_jit_gl_buffer_data*)`, with a
    /// count of descriptors, and that it synchronously copies everything it needs. It must
    /// not retain either the descriptor or its payload, or take ownership of their allocation.
    /// The buffer must describe valid, correctly sized/aligned data for this render context.
    /// These assumptions are not established by the public SDK headers.
    pub unsafe fn set_data_copying(&mut self, data: &mut GLDataBuffer<'_, '_>) {
        unsafe {
            sys::jit_object_method(
                self.as_ptr(),
                crate::sym!("setdata").as_ptr(),
                1 as c_long,
                data.as_ptr(),
            );
        }
    }
    /// # Safety
    /// All uploaded buffers must be initialized and valid for drawing. Any buffers retained by
    /// foreign methods must remain live and unmodified during the call.
    pub unsafe fn draw(&self) -> JitResult<()> {
        let code =
            unsafe { sys::jit_ob3d_draw_geometry(self.context.ob3d(), self.as_ptr().cast()) };
        if code == 0 { Ok(()) } else { Err(code as _) }
    }
}
impl Drop for GLGeometry<'_> {
    fn drop(&mut self) {
        unsafe {
            sys::jit_object_free(self.as_ptr());
        }
    }
}
