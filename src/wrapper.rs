//! External MaxObjWrappers.

use crate::{
    builder::{MSPWrappedBuilderInitial, MaxWrappedBuilder},
    class::{Class, ClassType, MaxFree},
    object::{MSPObj, MaxObj, ObjBox},
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

pub type MaxObjWrapper<T> = Wrapper<max_sys::t_object, MaxWrapperInternal<T>, T>;
pub type MSPObjWrapper<T> = Wrapper<max_sys::t_pxobject, MSPWrapperInternal<T>, T>;

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
    fn new(builder: &mut MaxWrappedBuilder<T>) -> Self;

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
    fn new(builder: &mut MSPWrappedBuilderInitial<T, MSPObjWrapper<T>>) -> Self;

    /// Perform DSP.
    fn perform(&self, ins: &[f64], outs: &mut [f64], nframes: usize);

    /// Register any methods you need for your class.
    fn class_setup(_class: &mut Class<MSPObjWrapper<Self>>) {
        //default, do nothing
    }
}

pub trait WrapperWrapped<T> {
    fn wrapped(&self) -> &T;
}

#[repr(C)]
pub struct Wrapper<O, I, T> {
    s_obj: O,
    wrapped: MaybeUninit<I>,
    _phantom: PhantomData<T>,
}

pub struct MaxWrapperInternal<T> {
    wrapped: T,
}

pub struct MSPWrapperInternal<T> {
    wrapped: T,
}

pub trait WrapperInternal<O, T>: Sized {
    fn wrapped(&self) -> &T;
    fn wrapped_mut(&mut self) -> &mut T;
    fn new(owner: *mut O) -> Self;
    fn class_setup(class: &mut Class<Wrapper<O, Self, T>>);
}

unsafe impl<I, T> MaxObj for Wrapper<max_sys::t_object, I, T> {}
unsafe impl<I, T> MaxObj for Wrapper<max_sys::t_pxobject, I, T> {}
unsafe impl<I, T> MSPObj for Wrapper<max_sys::t_pxobject, I, T> {}

impl<T> WrapperInternal<max_sys::t_object, T> for MaxWrapperInternal<T>
where
    T: MaxObjWrapped<T> + Send + Sync + 'static,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }
    fn wrapped_mut(&mut self) -> &mut T {
        &mut self.wrapped
    }
    fn new(owner: *mut max_sys::t_object) -> Self {
        let mut builder = MaxWrappedBuilder::new(owner);
        let wrapped = T::new(&mut builder);
        Self { wrapped }
    }
    fn class_setup(class: &mut Class<Wrapper<max_sys::t_object, Self, T>>) {
        T::class_setup(class);
    }
}

impl<T> WrapperInternal<max_sys::t_pxobject, T> for MSPWrapperInternal<T>
where
    T: MSPObjWrapped<T> + Send + Sync + 'static,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }
    fn wrapped_mut(&mut self) -> &mut T {
        &mut self.wrapped
    }
    fn new(owner: *mut max_sys::t_pxobject) -> Self {
        let mut builder = MSPWrappedBuilderInitial::new(owner);
        let wrapped = T::new(&mut builder);
        Self { wrapped }
    }
    fn class_setup(class: &mut Class<Wrapper<max_sys::t_pxobject, Self, T>>) {
        T::class_setup(class);
    }
}

fn new_common<F, O>(key: &'static str, func: F) -> O
where
    F: Fn(*mut max_sys::t_class) -> O,
{
    //unlock the mutex so we can register in the object init
    let max_class = {
        let g = CLASSES.lock().expect("couldn't lock CLASSES mutex");
        match g.get(key) {
            Some(class) => class.0,
            None => panic!("class {} not registered", key),
        }
    };
    func(max_class)
}

fn register_common<F, T>(key: &'static str, class_type: ClassType, creator: F)
where
    F: Fn() -> Class<T>,
{
    let mut h = CLASSES.lock().expect("couldn't lock CLASSES mutex");
    if !h.contains_key(key) {
        let mut c = creator();
        c.register(class_type)
            .expect(format!("failed to register {}", key).as_str());
        h.insert(key, ClassMaxObjWrapper(c.inner()));
    }
}

impl<O, I, T> WrapperWrapped<T> for Wrapper<O, I, T>
where
    I: WrapperInternal<O, T>,
    T: ObjWrapped<T>,
{
    /// Retrieve a reference to your wrapped class.
    fn wrapped(&self) -> &T {
        unsafe { (&*self.wrapped.as_ptr()).wrapped() }
    }
}

impl<O, I, T> Wrapper<O, I, T>
where
    I: WrapperInternal<O, T>,
    T: ObjWrapped<T>,
{
    /// Retrieve a mutable reference to your wrapped class.
    pub fn wrapped_mut(&mut self) -> &mut T {
        unsafe { (&mut *self.wrapped.as_mut_ptr()).wrapped_mut() }
    }

    extern "C" fn free_wrapped(&mut self) {
        //free wrapped
        let mut wrapped = MaybeUninit::uninit();
        std::mem::swap(&mut self.wrapped, &mut wrapped);
        unsafe {
            std::mem::drop(wrapped.assume_init());
        }
    }
}

fn key<T>() -> &'static str {
    std::any::type_name::<T>()
}

impl<T> Wrapper<max_sys::t_object, MaxWrapperInternal<T>, T>
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
        register_common(key::<T>(), T::class_type(), || {
            let mut c: Class<Self> = Class::new(
                T::class_name(),
                Self::new_tramp,
                Some(
                    std::mem::transmute::<extern "C" fn(&mut Self), MaxFree<Self>>(
                        Self::free_wrapped,
                    ),
                ),
            );
            //TODO somehow pass the lock so that classes can register additional classes
            MaxWrapperInternal::<T>::class_setup(&mut c);
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
        new_common(key::<T>(), |max_class| unsafe {
            let mut o: ObjBox<Self> = ObjBox::alloc(max_class);
            let internal = MaxWrapperInternal::<T>::new(o.max_obj());
            o.wrapped = MaybeUninit::new(internal);
            o
        })
    }
}

/*
impl<T> MSPObjWrapper<T>
where
    T: MSPObjWrapped<T> + Send + Sync + 'static,
{
    unsafe extern "C" fn free_msp(&mut self) {
        //free dsp first
        max_sys::z_dsp_free(self.msp_obj());
        //free wrapped
        let mut wrapped = MaybeUninit::uninit();
        std::mem::swap(&mut self.wrapped, &mut wrapped);
        std::mem::drop(wrapped.assume_init());
    }

    unsafe extern "C" fn perform64(
        &self,
        dsp64: *mut max_sys::t_object,
        ins: *const *const f64,
        numins: i64,
        outs: *const *mut f64,
        numouts: i64,
        sampleframes: i64,
        flags: i64,
        userparam: *mut c_void,
    ) {
        let ins = std::slice::from_raw_parts(ins, numins as _);
        let outs = std::slice::from_raw_parts(outs, numouts as _);
    }

    unsafe extern "C" fn dsp64(
        &self,
        dsp64: *mut max_sys::t_object,
        count: *mut std::os::raw::c_short,
        samplerate: f64,
        maxvectorsize: i64,
        flags: i64,
    ) {
    }

    /// Create an instance of the wrapper, on the heap.
    pub fn new() -> ObjBox<Self> {
        unimplemented!("asdf");
        /*
        unsafe {
            Self::new_common(|max_class| {
                let mut o: ObjBox<Self> = ObjBox::alloc(max_class);
                o.wrapped = MaybeUninit::new(MSPWrapperInternal::<T>::new(max_class, o.msp_obj()));
                o
            })
        }
        */
    }
}
*/

impl<O, I, T> Drop for Wrapper<O, I, T>
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
