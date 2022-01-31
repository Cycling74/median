#[repr(transparent)]
pub struct GLDataBuffer {
    inner: max_sys::t_jit_gl_buffer_data,
}

impl GLDataBuffer {
    pub fn as_ptr(&mut self) -> *mut max_sys::t_jit_gl_buffer_data {
        &mut self.inner as _
    }
}

impl Drop for GLDataBuffer {
    fn drop(&mut self) {
        unsafe {
            max_sys::jit_gl_buffer_data_destroy_tagged(&mut self.inner as _);
        }
    }
}
