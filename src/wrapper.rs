//! External Wrappers.

use crate::{
    class::{Class, ClassType, MaxFree},
    object::MaxObj,
};
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::sync::Mutex;

use lazy_static::lazy_static;
use std::collections::HashMap;

lazy_static! {
    //type name -> ClassWrapper
    static ref CLASSES: Mutex<HashMap<&'static str, ClassWrapper>> = Mutex::new(HashMap::new());
}

//we only use ClassWrapper in CLASSES after we've registered the class, for max's usage this is
//send
#[repr(transparent)]
struct ClassWrapper(*mut max_sys::t_class);
unsafe impl Send for ClassWrapper {}

pub trait Wrapped: Sized {
    fn new(o: *mut max_sys::t_object) -> Self;

    fn class_name() -> &'static str;

    /// Register any methods you need for your class
    fn class_setup(_class: &mut Class<Wrapper<Self>>) {
        //default, do nothing
    }
}

#[repr(C)]
pub struct Wrapper<T> {
    s_obj: max_sys::t_object,
    wrapped: MaybeUninit<T>,
}

unsafe impl<T> MaxObj for Wrapper<T> {}

impl<T> Wrapper<T>
where
    T: Wrapped + Send + Sync,
{
    pub fn wrapped_mut(&mut self) -> &mut T {
        unsafe { &mut (*self.wrapped.as_mut_ptr()) }
    }

    pub fn wrapped(&self) -> &T {
        unsafe { &(*self.wrapped.as_ptr()) }
    }

    // the key to use in the CLASSES hash
    fn key() -> &'static str {
        std::any::type_name::<Wrapper<T>>()
    }

    pub unsafe fn register(class_type: ClassType) {
        let mut c = Class::new(
            T::class_name(),
            Self::new_tramp,
            Some(std::mem::transmute::<extern "C" fn(&mut Self), MaxFree<Self>>(Self::free)),
        );
        T::class_setup(&mut c);
        c.register(class_type)
            .expect(format!("failed to register {}", Self::key()).as_str());
        CLASSES
            .lock()
            .expect("couldn't lock CLASSES mutex")
            .insert(Self::key(), ClassWrapper(c.inner()));
    }

    extern "C" fn free(&mut self) {
        let mut wrapped = MaybeUninit::uninit();
        std::mem::swap(&mut self.wrapped, &mut wrapped);
        unsafe {
            std::mem::drop(wrapped.assume_init());
        }
    }

    pub unsafe extern "C" fn new_tramp() -> *mut c_void {
        let g = CLASSES.lock().expect("couldn't lock CLASSES mutex");
        match g.get(Self::key()) {
            Some(class) => {
                let o = max_sys::object_alloc(class.0);
                let o = std::mem::transmute::<_, &mut Self>(o);
                o.init();
                std::mem::transmute::<_, _>(o)
            }
            None => panic!("class {} not registered", Self::key()),
        }
    }

    fn init(&mut self) {
        unsafe { self.wrapped = MaybeUninit::new(T::new(self.max_obj())) }
    }
}
