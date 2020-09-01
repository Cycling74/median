use crate::class::{Class, MaxFree, MaxNew};
use std::ffi::c_void;
use std::mem::MaybeUninit;

pub trait WrappedNew {
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

    pub extern "C" fn free(&mut self) {
        let mut wrapped = MaybeUninit::uninit();
        std::mem::swap(&mut self.wrapped, &mut wrapped);
        unsafe {
            std::mem::drop(wrapped.assume_init());
        }
    }

    pub fn maxobj(&mut self) -> *mut max_sys::t_object {
        &mut self.s_obj
    }

    //unfortunately the 'new' method needs to access a static variable
    //and so it cannot be created automatically
    //the result of `new_class` should be stored in that static
    //new should be a sample trampoline that calls Wrapper<T>::new(CLASS_STATIC.unwrap())
    pub fn new_class(name: &str, new: MaxNew) -> Class<Self> {
        unsafe {
            Class::new(
                name,
                new,
                Some(std::mem::transmute::<extern "C" fn(&mut Self), MaxFree<Self>>(Self::free)),
            )
        }
    }
}

impl<T> Wrapper<T>
where
    T: WrappedNew,
{
    pub fn new(class: &mut Option<Class<Wrapper<T>>>) -> *mut c_void {
        match class {
            Some(class) => unsafe {
                let o = max_sys::object_alloc(class.inner());
                let o = std::mem::transmute::<_, &mut Self>(o);
                o.init();
                std::mem::transmute::<_, _>(o)
            },
            None => panic!("class not registered"),
        }
    }
    pub fn init(&mut self) {
        self.wrapped = MaybeUninit::new(T::new(self.maxobj()))
    }
}
