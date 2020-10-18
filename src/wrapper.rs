//! External MaxObjWrappers.

use crate::{
    class::{Class, ClassType, MaxFree},
    clock::ClockHandle,
    object::{MaxObj, ObjBox},
};

use std::collections::HashMap;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::sync::Mutex;

use lazy_static::lazy_static;

lazy_static! {
    //type name -> ClassMaxObjWrapper
    static ref CLASSES: Mutex<HashMap<&'static str, ClassMaxObjWrapper>> = Mutex::new(HashMap::new());
}

//we only use ClassMaxObjWrapper in CLASSES after we've registered the class, for max's usage this is
//send
#[repr(transparent)]
struct ClassMaxObjWrapper(*mut max_sys::t_class);
unsafe impl Send for ClassMaxObjWrapper {}

pub trait MaxObjWrappedBuilder<T> {
    /// Create a clock with a method callback
    fn with_clockfn(&mut self, func: fn(&T)) -> ClockHandle;
    /// Create a clock with a closure callback
    fn with_clock(&mut self, func: Box<dyn Fn(&T)>) -> ClockHandle;

    /// Get the parent max object which can be cast to `&MaxObjWrapper<T>`.
    /// This in turn can be used to get your object with the `wrapped()` method.
    unsafe fn wrapper(&mut self) -> *mut max_sys::t_object;
}

pub trait MaxObjWrapped<T>: Sized {
    /// A constructor for your object.
    ///
    /// # Arguments
    ///
    /// * `parent` - The max `t_object` that owns this wrapped object. Can be used to create
    /// inlets/outlets etc.
    fn new(builder: &mut dyn MaxObjWrappedBuilder<T>) -> Self;

    /// The name of your class, this is what you'll type into a box in Max if your class is a
    /// `ClassType::Box`.
    ///
    /// You can add additional aliases in the `class_setup` method.
    fn class_name() -> &'static str;

    /// The type of your class. Defaults to 'box' which creates visual objects in Max.
    fn class_type() -> ClassType {
        ClassType::Box
    }

    /// Register any methods you need for your class.
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        //default, do nothing
    }
}

/// The actual struct that Max gets.
/// Users shouldn't need to interact with this.
#[repr(C)]
pub struct MaxObjWrapper<T> {
    s_obj: max_sys::t_object,
    wrapped: MaybeUninit<T>,
}

unsafe impl<T> MaxObj for MaxObjWrapper<T> {}

impl<T> MaxObjWrapper<T>
where
    T: MaxObjWrapped<T> + Send + Sync + 'static,
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
        std::any::type_name::<MaxObjWrapper<T>>()
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
            h.insert(key, ClassMaxObjWrapper(c.inner()));
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
    /// XXX THIS crashes when we dealloc.. the memory is from max not from Box::new.. need to
    /// figure that out
    pub fn new() -> ObjBox<Self> {
        unsafe {
            //unlock the mutex so we can register in the object init
            let max_class = {
                let g = CLASSES.lock().expect("couldn't lock CLASSES mutex");
                match g.get(Self::key()) {
                    Some(class) => class.0,
                    None => panic!("class {} not registered", Self::key()),
                }
            };
            let mut o: ObjBox<Self> = ObjBox::alloc(max_class);
            o.init();
            o
        }
    }

    /// A method for Max to create an instance of your class.
    pub unsafe extern "C" fn new_tramp() -> *mut c_void {
        let o = ObjBox::into_raw(Self::new());
        std::mem::transmute::<_, _>(o)
    }

    fn init(&mut self) {
        unsafe {
            let mut builder = Builder::new(self.max_obj());
            self.wrapped = MaybeUninit::new(T::new(&mut builder))
        }
    }
}

pub struct Builder<T> {
    wrapper: *mut max_sys::t_object,
    _phantom: PhantomData<T>,
}

impl<T> Builder<T> {
    pub fn new(wrapper: *mut max_sys::t_object) -> Self {
        Self {
            wrapper,
            _phantom: PhantomData,
        }
    }
}

impl<T> MaxObjWrappedBuilder<T> for Builder<T>
where
    T: MaxObjWrapped<T> + Send + Sync + 'static,
{
    /// Create a clock with a method callback
    fn with_clockfn(&mut self, func: fn(&T)) -> ClockHandle {
        unsafe {
            ClockHandle::new(
                // XXX wrapper should outlive the ClockHandle, but we haven't guaranteed that..
                self.wrapper,
                Box::new(move |wrapper| {
                    let wrapper: &MaxObjWrapper<T> =
                        std::mem::transmute::<_, &MaxObjWrapper<T>>(wrapper);
                    func(wrapper.wrapped());
                }),
            )
        }
    }

    /// Create a clock with a closure callback
    fn with_clock(&mut self, func: Box<dyn Fn(&T)>) -> ClockHandle {
        unsafe {
            ClockHandle::new(
                // XXX wrapper should outlive the ClockHandle, but we haven't guaranteed that..
                self.wrapper,
                Box::new(move |wrapper| {
                    let wrapper: &MaxObjWrapper<T> =
                        std::mem::transmute::<_, &MaxObjWrapper<T>>(wrapper);
                    func(wrapper.wrapped());
                }),
            )
        }
    }

    /// Get the parent max object which can be cast to `&MaxObjWrapper<T>`.
    /// This in turn can be used to get your object with the `wrapped()` method.
    unsafe fn wrapper(&mut self) -> *mut max_sys::t_object {
        self.wrapper
    }
}

impl<T> Drop for MaxObjWrapper<T>
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
