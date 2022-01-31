use super::buffer::GLDataBuffer;
use std::ffi::c_void;

pub struct GLGeometry {
    inner: *mut c_void,
}

impl GLGeometry {
    pub fn set_data(&self, mut data: GLDataBuffer) {
        unsafe {
            max_sys::jit_object_method(
                self.inner,
                crate::max::common_symbols().s_setdata,
                1,
                data.as_ptr(),
            );
        }
    }
}

impl Drop for GLGeometry {
    fn drop(&mut self) {
        unsafe {
            max_sys::jit_object_free(self.inner);
        }
    }
}
