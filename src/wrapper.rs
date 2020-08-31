use crate::class::Class;
use std::ffi::c_void;
use std::mem::MaybeUninit;

pub trait WrapperNew {
    fn new(o: *mut max_sys::t_object) -> Self;
}

#[repr(C)]
pub struct Wrapper<T> {
    s_obj: max_sys::t_object,
    wrapped: MaybeUninit<T>,
}

impl<T> Wrapper<T> {
    pub fn wrapped(&mut self) -> &mut T {
        unsafe { &mut (*self.wrapped.as_mut_ptr()) }
    }
    pub fn free(&mut self) {
        let mut wrapped = MaybeUninit::uninit();
        std::mem::swap(&mut self.wrapped, &mut wrapped);
        unsafe {
            std::mem::drop(wrapped.assume_init());
        }
    }
    pub fn maxobj(&mut self) -> *mut max_sys::t_object {
        &mut self.s_obj
    }
}

impl<T> Wrapper<T>
where
    T: WrapperNew,
{
    pub fn new(class: &mut Class<T>) -> *mut c_void {
        unsafe {
            let o = max_sys::object_alloc(class.inner());
            let o = std::mem::transmute::<_, &mut Self>(o);
            o.init();
            std::mem::transmute::<_, _>(o)
        }
    }
    pub fn init(&mut self) {
        self.wrapped = MaybeUninit::new(T::new(self.maxobj()))
    }
}
