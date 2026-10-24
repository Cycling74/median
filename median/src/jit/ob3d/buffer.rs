use crate::jit::DrawContext;
use max_sys as sys;
use std::marker::PhantomData;

/// Own SDK-created buffer-view metadata while borrowing its caller-owned payload. The SDK
/// descriptor can only be adopted unsafely until its allocation/retention contracts are verified.
pub struct GLDataBuffer<'ctx, 'data> {
    raw: sys::t_jit_gl_buffer_data,
    _context: &'ctx DrawContext,
    _data: PhantomData<&'data mut [u8]>,
}
impl<'ctx, 'data> GLDataBuffer<'ctx, 'data> {
    /// # Safety
    /// Transfer the sole descriptor created by a `jit_gl_buffer_data_create_*` routine that
    /// pairs with `jit_gl_buffer_data_destroy_tagged`. Its view metadata must be owned, valid
    /// and unshared. Its payload must remain valid for `'data` and the descriptor must not
    /// outlive it. Verify that destruction releases only descriptor-owned metadata, not the
    /// borrowed payload. No foreign code may retain pointers past this handle's lifetime.
    pub unsafe fn from_raw(context: &'ctx DrawContext, raw: sys::t_jit_gl_buffer_data) -> Self {
        Self {
            raw,
            _context: context,
            _data: PhantomData,
        }
    }
    pub fn as_ptr(&mut self) -> *mut sys::t_jit_gl_buffer_data {
        &mut self.raw
    }
    /// Transfer descriptor cleanup responsibility back to the caller.
    pub fn into_raw(self) -> sys::t_jit_gl_buffer_data {
        let raw = self.raw;
        std::mem::forget(self);
        raw
    }
}
impl Drop for GLDataBuffer<'_, '_> {
    fn drop(&mut self) {
        unsafe {
            sys::jit_gl_buffer_data_destroy_tagged(&mut self.raw);
        }
    }
}
