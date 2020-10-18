//! External MaxObjWrappers.

use crate::{
    builder::{MSPWrappedBuilder, MaxWrappedBuilder, WrappedBuilder},
    class::{Class, ClassType, MaxFree},
    object::{MSPObj, MaxObj, ObjBox},
};

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::sync::Mutex;

use lazy_static::lazy_static;

lazy_static! {
    //type name -> ClassMaxObjWrapper
    static ref CLASSES: Mutex<HashMap<&'static str, ClassMaxObjWrapper>> = Mutex::new(HashMap::new());
}

//we only use ClassMaxObjWrapper in CLASSES after we've registered the class, for max's usage this is
//Send
#[repr(transparent)]
struct ClassMaxObjWrapper(*mut max_sys::t_class);
unsafe impl Send for ClassMaxObjWrapper {}

pub trait ObjWrapped<T>: Sized {
    /// The name of your class, this is what you'll type into a box in Max if your class is a
    /// `ClassType::Box`.
    ///
    /// You can add additional aliases in the `class_setup` method.
    fn class_name() -> &'static str;

    /// The type of your class. Defaults to 'box' which creates visual objects in Max.
    fn class_type() -> ClassType {
        ClassType::Box
    }
}

pub trait MaxObjWrapped<T>: ObjWrapped<T> {
    /// A constructor for your object.
    ///
    /// # Arguments
    ///
    /// * `builder` - A builder for constructing inlets/oulets/etc.
    fn new(builder: &mut dyn MaxWrappedBuilder<T>) -> Self;

    /// Register any methods you need for your class.
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        //default, do nothing
    }
}

pub trait MSPObjWrapped<T>: ObjWrapped<T> {
    /// A constructor for your object.
    ///
    /// # Arguments
    ///
    /// * `builder` - A builder for constructing inlets/oulets/etc.
    fn new(builder: &mut dyn MSPWrappedBuilder<T>) -> Self;

    /// Perform DSP.
    fn perform(&self, ins: &[f64], outs: &mut [f64], nframes: usize);

    /// Register any methods you need for your class.
    fn class_setup(_class: &mut Class<MSPObjWrapper<Self>>) {
        //default, do nothing
    }
}

/// The actual struct that Max gets.
/// Users shouldn't need to interact with this.
#[repr(C)]
pub struct ObjWrapper<Obj, T> {
    s_obj: Obj,
    wrapped: MaybeUninit<T>,
}

pub type MaxObjWrapper<T> = ObjWrapper<max_sys::t_object, T>;
pub type MSPObjWrapper<T> = ObjWrapper<max_sys::t_pxobject, T>;

unsafe impl<T> MaxObj for MaxObjWrapper<T> {}
unsafe impl<T> MaxObj for MSPObjWrapper<T> {}
unsafe impl<T> MSPObj for MSPObjWrapper<T> {}

impl<Obj, T> ObjWrapper<Obj, T>
where
    T: ObjWrapped<T> + Send + Sync + 'static,
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

    unsafe extern "C" fn free_wrapped(&mut self) {
        //free wrapped
        let mut wrapped = MaybeUninit::uninit();
        std::mem::swap(&mut self.wrapped, &mut wrapped);
        std::mem::drop(wrapped.assume_init());
    }

    fn new_common<F, O>(func: F) -> O
    where
        F: Fn(*mut max_sys::t_class) -> O,
    {
        //unlock the mutex so we can register in the object init
        let max_class = {
            let g = CLASSES.lock().expect("couldn't lock CLASSES mutex");
            match g.get(Self::key()) {
                Some(class) => class.0,
                None => panic!("class {} not registered", Self::key()),
            }
        };
        func(max_class)
    }

    unsafe fn register_common<F>(creator: F)
    where
        F: Fn() -> Class<ObjWrapper<Obj, T>>,
    {
        let mut h = CLASSES.lock().expect("couldn't lock CLASSES mutex");
        let key = Self::key();
        if !h.contains_key(key) {
            let mut c = creator();
            c.register(T::class_type())
                .expect(format!("failed to register {}", Self::key()).as_str());
            h.insert(key, ClassMaxObjWrapper(c.inner()));
        }
    }
}

impl<T> MaxObjWrapper<T>
where
    T: MaxObjWrapped<T> + Send + Sync + 'static,
{
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
        Self::register_common(|| {
            let mut c = Class::new(
                T::class_name(),
                Self::new_tramp,
                Some(std::mem::transmute::<
                    unsafe extern "C" fn(&mut Self),
                    MaxFree<Self>,
                >(Self::free_wrapped)),
            );
            //TODO somehow pass the lock so that classes can register additional classes
            T::class_setup(&mut c);
            c
        });
    }

    /// A method for Max to create an instance of your class.
    pub unsafe extern "C" fn new_tramp() -> *mut c_void {
        let o = ObjBox::into_raw(Self::new());
        std::mem::transmute::<_, _>(o)
    }

    /// Create an instance of the wrapper, on the heap.
    pub fn new() -> ObjBox<Self> {
        unsafe {
            Self::new_common(|max_class| {
                let mut o: ObjBox<Self> = ObjBox::alloc(max_class);
                o.init();
                o
            })
        }
    }

    unsafe fn init(&mut self) {
        let mut builder = WrappedBuilder::new(self);
        self.wrapped = MaybeUninit::new(T::new(&mut builder))
    }
}

impl<T> MSPObjWrapper<T>
where
    T: MSPObjWrapped<T> + Send + Sync + 'static,
{
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
        Self::register_common(|| {
            let mut c = Class::new(
                T::class_name(),
                Self::new_tramp,
                Some(std::mem::transmute::<
                    unsafe extern "C" fn(&mut Self),
                    MaxFree<Self>,
                >(Self::free_msp)),
            );
            //TODO somehow pass the lock so that classes can register additional classes
            T::class_setup(&mut c);
            c
        });
    }

    /// A method for Max to create an instance of your class.
    pub unsafe extern "C" fn new_tramp() -> *mut c_void {
        let o = ObjBox::into_raw(Self::new());
        std::mem::transmute::<_, _>(o)
    }

    unsafe extern "C" fn free_msp(&mut self) {
        //free dsp first
        max_sys::z_dsp_free(self.msp_obj());
        self.free_wrapped();
    }

    /// Create an instance of the wrapper, on the heap.
    pub fn new() -> ObjBox<Self> {
        unsafe {
            Self::new_common(|max_class| {
                let mut o: ObjBox<Self> = ObjBox::alloc(max_class);
                o.init();
                o
            })
        }
    }

    unsafe fn init(&mut self) {
        let mut builder = WrappedBuilder::new(self);
        self.wrapped = MaybeUninit::new(T::new(&mut builder));
    }
}

impl<Obj, T> Drop for ObjWrapper<Obj, T>
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
