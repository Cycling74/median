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
    /// A constructor for your object.
    ///
    /// # Arguments
    ///
    /// * `parent` - The max `t_object` that owns this wrapped object. Can be used to create
    /// inlets/outlets etc.
    fn new(parent: *mut max_sys::t_object) -> Self;

    /// The name of your class, this is what you'll type into a box in Max if your class is a
    /// `ClassType::Box`.
    fn class_name() -> &'static str;

    /// The type of your class. Defaults to 'box' which creates visual objects in Max.
    fn class_type() -> ClassType {
        ClassType::Box
    }

    /// Register any methods you need for your class.
    fn class_setup(_class: &mut Class<Wrapper<Self>>) {
        //default, do nothing
    }
}

/// The actual struct that Max gets.
/// Users shouldn't need to interact with this.
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
    /// Retrieve a mutable reference to your wrapped class.
    pub fn wrapped_mut(&mut self) -> &mut T {
        unsafe { &mut (*self.wrapped.as_mut_ptr()) }
    }

    /// Retrieve a reference to your wrapped class.
    pub fn wrapped(&self) -> &T {
        unsafe { &(*self.wrapped.as_ptr()) }
    }

    // the key to use in the CLASSES hash
    fn key() -> &'static str {
        std::any::type_name::<Wrapper<T>>()
    }

    /// Register the class with Max.
    ///
    /// # Remarks
    ///
    /// This method expects to only be called from the main thread. Internally, it locks a mutex
    /// and looks up your class by type name. If your class has alrady been registered it won't
    /// re-register.
    ///
    /// This will deadlock if you call `register()` again inside your `T::class_setup()`.
    pub unsafe fn register() {
        let mut h = CLASSES.lock().expect("couldn't lock CLASSES mutex");
        let key = Self::key();
        if !h.contains_key(key) {
            let mut c = Class::new(
                T::class_name(),
                Self::new_tramp,
                Some(std::mem::transmute::<extern "C" fn(&mut Self), MaxFree<Self>>(Self::free)),
            );
            //TODO somehow pass the lock so that classes can register additional classes
            T::class_setup(&mut c);
            c.register(T::class_type())
                .expect(format!("failed to register {}", Self::key()).as_str());
            h.insert(key, ClassWrapper(c.inner()));
        }
    }

    extern "C" fn free(&mut self) {
        let mut wrapped = MaybeUninit::uninit();
        std::mem::swap(&mut self.wrapped, &mut wrapped);
        unsafe {
            std::mem::drop(wrapped.assume_init());
        }
    }

    /// Create an instance of the wrapper, on the heap.
    pub fn new() -> Box<Self> {
        unsafe {
            //unlock the mutex so we can register in the object init
            let max_class = {
                let g = CLASSES.lock().expect("couldn't lock CLASSES mutex");
                match g.get(Self::key()) {
                    Some(class) => class.0,
                    None => panic!("class {} not registered", Self::key()),
                }
            };
            let o = max_sys::object_alloc(max_class);
            let o = std::mem::transmute::<_, &mut Self>(o);
            o.init();
            std::boxed::Box::from_raw(o)
        }
    }

    /// A method for Max to create an instance of your class.
    pub unsafe extern "C" fn new_tramp() -> *mut c_void {
        let o = Box::into_raw(Self::new());
        std::mem::transmute::<_, _>(o)
    }

    fn init(&mut self) {
        unsafe { self.wrapped = MaybeUninit::new(T::new(self.max_obj())) }
    }
}

impl<T> Drop for Wrapper<T>
where
    T: Sized,
{
    fn drop(&mut self) {
        unsafe {
            //use Max's object_free which will call the wrapper's "free" method.
            max_sys::object_free(std::mem::transmute::<_, _>(&self.s_obj));
        }
    }
}
