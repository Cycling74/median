/// Indicates that your struct can be safely cast to a max_sys::t_object this means your struct
/// must be `#[repr(C)]` and have a `max_sys::t_object` as its first member.
pub unsafe trait MaxObj: Sized {
    unsafe fn max_obj(&mut self) -> *mut max_sys::t_object {
        std::mem::transmute::<_, *mut max_sys::t_object>(self)
    }
}

/// Indicates that your struct can be safely cast to a max_sys::t_pxobject this means your struct
/// must be `#[repr(C)]` and have a `max_sys::t_pxobject` as its first member.
///
/// This automatically implements MaxObj.
pub unsafe trait MSPObj: Sized {
    unsafe fn msp_obj(&mut self) -> *mut max_sys::t_pxobject {
        std::mem::transmute::<_, *mut max_sys::t_pxobject>(self)
    }
}

unsafe impl<T> MaxObj for T where T: MSPObj {}
