//! External MaxObjWrappers.

use crate::{
    atom::Atom,
    buffer::BufferRef,
    builder::{MSPWrappedBuilder, ManagedBufferRefInternal, MaxWrappedBuilder, WrappedBuilder},
    class::{Class, ClassType},
    inlet::{FloatCB, IntCB},
    method::{MaxFree, MaxMethod},
    notify::Notification,
    object::{MSPObj, MaxObj, ObjBox, Subscription},
    symbol::SymbolRef,
};

use std::{
    collections::{HashMap, HashSet},
    ffi::{c_void, CString},
    marker::PhantomData,
    mem::MaybeUninit,
    sync::{Arc, Mutex, Weak},
};

use lazy_static::lazy_static;

lazy_static! {
    //type name -> ClassMaxObjWrapper
    static ref CLASSES: Mutex<HashMap<&'static str, ClassMaxObjWrapper>> = Mutex::new(HashMap::new());
}

pub type MaxObjWrapper<T> = Wrapper<max_sys::t_object, MaxWrapperInternal<T>, T>;
pub type MSPObjWrapper<T> = Wrapper<max_sys::t_pxobject, MSPWrapperInternal<T>, T>;

pub type FloatCBHash<T> = HashMap<usize, FloatCB<T>>;
pub type IntCBHash<T> = HashMap<usize, IntCB<T>>;

pub type DeferMethodWrapped<T> = extern "C" fn(
    wrapper: &T,
    sym: *mut max_sys::t_symbol,
    argc: std::os::raw::c_long,
    argv: *const max_sys::t_atom,
);

//reexports
pub use median_macros::wrapped_attr_get_tramp as attr_get_tramp;
pub use median_macros::wrapped_attr_set_tramp as attr_set_tramp;
pub use median_macros::wrapped_defer_tramp as defer_tramp;
pub use median_macros::wrapped_tramp as tramp;

//we only use ClassMaxObjWrapper in CLASSES after we've registered the class, for max's usage this is
//Send
#[repr(transparent)]
struct ClassMaxObjWrapper(*mut max_sys::t_class);
unsafe impl Send for ClassMaxObjWrapper {}

pub trait ObjWrapped<T>: Sized + Sync + 'static {
    /// The name of your class, this is what you'll type into a box in Max if your class is a
    /// `ClassType::Box`.
    ///
    /// You can add additional aliases in the `class_setup` method.
    fn class_name() -> &'static str;

    /// The type of your class. Defaults to 'box' which creates visual objects in Max.
    fn class_type() -> ClassType {
        ClassType::Box
    }

    /// Handle notifications that your object gets
    fn handle_notification(&self, _notification: &Notification) {}
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
    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], nframes: usize);

    /// Register any methods you need for your class.
    fn class_setup(_class: &mut Class<MSPObjWrapper<Self>>) {
        //default, do nothing
    }
}

pub trait WrapperWrapped<T> {
    /// Retrieve a reference to your wrapped class.
    fn wrapped(&self) -> &T;
}

/// Defer methods for wrapped objects.
pub trait WrappedDefer<T> {
    ///defer a tramp method with the sym and atoms args
    fn defer(&self, method: DeferMethodWrapped<T>, sym: SymbolRef, atoms: &[Atom]);
    ///defer_low a tramp method with the sym and atoms args
    fn defer_low(&self, method: DeferMethodWrapped<T>, sym: SymbolRef, atoms: &[Atom]);
}

/// Attachements for notifications from other objects.
pub trait WrappedAttach<T> {
    ///attempt to attach to an object with the given name in the given namespace.
    fn attach(&self, namespace: SymbolRef, name: SymbolRef) -> Result<WrappedAttachmentHandle, ()>;

    ///detach from the object with the given handle.
    fn detach(&self, handle: WrappedAttachmentHandle);

    ///subscribe to attach to an object with the given name in the given namespace.
    fn subscribe(
        &self,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> WrappedSubscriptionHandle;

    ///unsubscribe from the subscription.
    fn unsubscribe(&self, handle: WrappedSubscriptionHandle);
}

#[derive(PartialEq, Eq, Hash)]
pub(crate) struct WrappedAttachment {
    namespace: SymbolRef,
    name: SymbolRef,
    client: *mut core::ffi::c_void,
}

/// A handle for an attachment, used to detatch.
pub struct WrappedAttachmentHandle {
    inner: Weak<WrappedAttachment>,
}

/// A handle for an subscription, used to unsubscribe.
pub struct WrappedSubscriptionHandle {
    inner: Weak<Subscription>,
}

#[repr(C)]
pub struct Wrapper<O, I, T> {
    s_obj: O,
    wrapped: MaybeUninit<I>,
    _phantom: PhantomData<T>,
}

pub struct MaxWrapperInternal<T> {
    wrapped: T,
    callbacks_float: FloatCBHash<T>,
    callbacks_int: IntCBHash<T>,
    buffer_refs: Vec<ManagedBufferRefInternal>,
    //we just hold onto these so they don't get deallocated until later
    _proxy_inlets: Vec<crate::inlet::Proxy>,
    attachments: Mutex<HashSet<Arc<WrappedAttachment>>>,
    subscriptions: Mutex<HashSet<Arc<Subscription>>>,
}

pub struct MSPWrapperInternal<T> {
    wrapped: T,
    ins: Vec<&'static [f64]>,
    outs: Vec<&'static mut [f64]>,
    callbacks_float: FloatCBHash<T>,
    callbacks_int: IntCBHash<T>,
    buffer_refs: Vec<ManagedBufferRefInternal>,
    //we just hold onto these so they don't get deallocated until later
    _proxy_inlets: Vec<crate::inlet::Proxy>,
    attachments: Mutex<HashSet<Arc<WrappedAttachment>>>,
    subscriptions: Mutex<HashSet<Arc<Subscription>>>,
}

pub trait WrapperInternal<O, T>: Sized {
    fn wrapped(&self) -> &T;
    fn wrapped_mut(&mut self) -> &mut T;
    fn new(owner: *mut O, sym: SymbolRef, args: &[Atom]) -> Self;
    fn class_setup(class: &mut Class<Wrapper<O, Self, T>>);

    fn call_float(&self, index: usize, value: f64);
    fn call_int(&self, index: usize, value: i64);

    fn handle_notification(&self, notification: &Notification);

    fn attach(
        &self,
        client: *mut max_sys::t_object,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<WrappedAttachmentHandle, ()>;
    fn detatch(&self, handle: WrappedAttachmentHandle);
    fn subscribe(
        &self,
        client: *mut max_sys::t_object,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> WrappedSubscriptionHandle;
    fn unsubscribe(&self, handle: WrappedSubscriptionHandle);
}

unsafe impl<I, T> MaxObj for Wrapper<max_sys::t_object, I, T> {}
unsafe impl<I, T> MaxObj for Wrapper<max_sys::t_pxobject, I, T> {}
unsafe impl<I, T> MSPObj for Wrapper<max_sys::t_pxobject, I, T> {}

impl<T> WrapperInternal<max_sys::t_object, T> for MaxWrapperInternal<T>
where
    T: MaxObjWrapped<T> + Sync + 'static,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }
    fn wrapped_mut(&mut self) -> &mut T {
        &mut self.wrapped
    }
    fn new(owner: *mut max_sys::t_object, sym: SymbolRef, args: &[Atom]) -> Self {
        let mut builder = WrappedBuilder::new_max(owner, sym, args);
        let wrapped = T::new(&mut builder);
        let mut f = builder.finalize();
        Self {
            wrapped,
            callbacks_float: std::mem::take(&mut f.callbacks_float),
            callbacks_int: std::mem::take(&mut f.callbacks_int),
            buffer_refs: std::mem::take(&mut f.buffer_refs),
            _proxy_inlets: std::mem::take(&mut f.proxy_inlets),
            attachments: std::mem::take(&mut f.attachments),
            subscriptions: std::mem::take(&mut f.subscriptions),
        }
    }
    fn class_setup(class: &mut Class<Wrapper<max_sys::t_object, Self, T>>) {
        T::class_setup(class);
    }
    fn call_float(&self, index: usize, value: f64) {
        if let Some(f) = self.callbacks_float.get(&index) {
            f(self.wrapped(), value);
        }
    }
    fn call_int(&self, index: usize, value: i64) {
        if let Some(f) = self.callbacks_int.get(&index) {
            f(self.wrapped(), value);
        }
    }
    fn handle_notification(&self, notification: &Notification) {
        handle_buffer_ref_notifications(&self.buffer_refs, notification);
        self.wrapped().handle_notification(notification);
    }
    fn attach(
        &self,
        client: *mut max_sys::t_object,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<WrappedAttachmentHandle, ()> {
        let mut g = self.attachments.lock().unwrap();
        match WrappedAttachment::new(client, namespace, name) {
            Ok(inner) => {
                g.insert(inner.clone());
                Ok(WrappedAttachmentHandle::new(&inner))
            }
            Err(e) => Err(e),
        }
    }
    fn detatch(&self, handle: WrappedAttachmentHandle) {
        let mut g = self.attachments.lock().unwrap();
        if let Some(handle) = handle.inner.upgrade() {
            g.remove(&handle);
        }
    }
    fn subscribe(
        &self,
        client: *mut max_sys::t_object,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> WrappedSubscriptionHandle {
        let s = Arc::new(Subscription::new(client, namespace, name, class_name));
        let mut g = self.subscriptions.lock().unwrap();
        g.insert(s.clone());
        WrappedSubscriptionHandle::new(&s)
    }
    fn unsubscribe(&self, handle: WrappedSubscriptionHandle) {
        let mut g = self.subscriptions.lock().unwrap();
        if let Some(handle) = handle.inner.upgrade() {
            g.remove(&handle);
        }
    }
}

impl<T> WrapperInternal<max_sys::t_pxobject, T> for MSPWrapperInternal<T>
where
    T: MSPObjWrapped<T> + Sync + 'static,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }
    fn wrapped_mut(&mut self) -> &mut T {
        &mut self.wrapped
    }
    fn new(owner: *mut max_sys::t_pxobject, sym: SymbolRef, args: &[Atom]) -> Self {
        let mut builder = WrappedBuilder::new_msp(owner, sym, args);
        let wrapped = T::new(&mut builder);
        let mut f = builder.finalize();
        let ins = (0..f.signal_inlets)
            .map(|_i| unsafe { std::slice::from_raw_parts(std::ptr::null(), 0) })
            .collect();
        let outs: Vec<&'static mut [f64]> = (0..f.signal_outlets)
            .map(|_i| unsafe { std::slice::from_raw_parts_mut(std::ptr::null_mut(), 0) })
            .collect();
        Self {
            wrapped,
            ins,
            outs,
            callbacks_float: std::mem::take(&mut f.callbacks_float),
            callbacks_int: std::mem::take(&mut f.callbacks_int),
            buffer_refs: std::mem::take(&mut f.buffer_refs),
            _proxy_inlets: std::mem::take(&mut f.proxy_inlets),
            attachments: std::mem::take(&mut f.attachments),
            subscriptions: std::mem::take(&mut f.subscriptions),
        }
    }
    fn class_setup(class: &mut Class<Wrapper<max_sys::t_pxobject, Self, T>>) {
        T::class_setup(class);
    }
    fn call_float(&self, index: usize, value: f64) {
        if let Some(f) = self.callbacks_float.get(&index) {
            f(self.wrapped(), value);
        }
    }
    fn call_int(&self, index: usize, value: i64) {
        if let Some(f) = self.callbacks_int.get(&index) {
            f(self.wrapped(), value);
        }
    }
    fn handle_notification(&self, notification: &Notification) {
        handle_buffer_ref_notifications(&self.buffer_refs, notification);
        self.wrapped().handle_notification(notification);
    }
    fn attach(
        &self,
        client: *mut max_sys::t_object,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<WrappedAttachmentHandle, ()> {
        let mut g = self.attachments.lock().unwrap();
        match WrappedAttachment::new(client, namespace, name) {
            Ok(inner) => {
                g.insert(inner.clone());
                Ok(WrappedAttachmentHandle::new(&inner))
            }
            Err(e) => Err(e),
        }
    }
    fn detatch(&self, handle: WrappedAttachmentHandle) {
        let mut g = self.attachments.lock().unwrap();
        if let Some(handle) = handle.inner.upgrade() {
            g.remove(&handle);
        }
    }
    fn subscribe(
        &self,
        client: *mut max_sys::t_object,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> WrappedSubscriptionHandle {
        let s = Arc::new(Subscription::new(client, namespace, name, class_name));
        let mut g = self.subscriptions.lock().unwrap();
        g.insert(s.clone());
        WrappedSubscriptionHandle::new(&s)
    }
    fn unsubscribe(&self, handle: WrappedSubscriptionHandle) {
        let mut g = self.subscriptions.lock().unwrap();
        if let Some(handle) = handle.inner.upgrade() {
            g.remove(&handle);
        }
    }
}

fn handle_buffer_ref_notifications(
    buffer_refs: &Vec<ManagedBufferRefInternal>,
    notification: &Notification,
) {
    if BufferRef::is_applicable(notification) {
        for r in buffer_refs {
            unsafe {
                r.notify_if_unchecked(&notification);
            }
        }
    }
}

impl<T> MSPWrapperInternal<T>
where
    T: MSPObjWrapped<T> + Sync + 'static,
{
    extern "C" fn perform64(
        &mut self,
        _dsp64: *mut max_sys::t_object,
        ins: *const *const f64,
        numins: i64,
        outs: *mut *mut f64,
        numouts: i64,
        sampleframes: i64,
        _flags: i64,
        _userparam: *mut c_void,
    ) {
        assert!(self.ins.len() >= numins as _);
        assert!(self.outs.len() >= numouts as _);
        let nframes = sampleframes as usize;

        //convert into slices
        let ins = unsafe { std::slice::from_raw_parts(ins, numins as _) };
        for (i, ip) in self.ins.iter_mut().zip(ins) {
            unsafe {
                *i = std::slice::from_raw_parts(*ip, nframes);
            }
        }
        let outs = unsafe { std::slice::from_raw_parts_mut(outs, numouts as _) };
        for (o, op) in self.outs.iter_mut().zip(outs) {
            unsafe {
                *o = std::slice::from_raw_parts_mut(*op, nframes);
            }
        }

        //do a dance so we can access an immutable and a mutable at the same time
        let mut ins = std::mem::take(&mut self.ins);
        let mut outs = std::mem::take(&mut self.outs);
        self.wrapped()
            .perform(ins.as_slice(), outs.as_mut_slice(), nframes);
        std::mem::swap(&mut self.ins, &mut ins);
        std::mem::swap(&mut self.outs, &mut outs);
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

impl<O, I, T> WrapperWrapped<T> for Wrapper<O, I, T>
where
    I: WrapperInternal<O, T>,
    T: ObjWrapped<T>,
{
    fn wrapped(&self) -> &T {
        self.internal().wrapped()
    }
}

//build up our float and int input trampolines, and the register fn
macro_rules! int_float_tramps {
    ( $( $i:literal ),+ ) => {
        $(
            paste::paste! {
                pub extern "C" fn [<call_in $i>](&self, value: i64) {
                    self.internal().call_int($i, value);
                }

                pub extern "C" fn [<call_ft $i>](&self, value: f64) {
                    self.internal().call_float($i, value);
                }
            }
        )*

        fn register_ft_in(class: *mut max_sys::t_class) {
            unsafe {
            $(
                paste::paste! {
                    max_sys::class_addmethod(class,
                        Some(std::mem::transmute::<extern "C" fn(&Self, f64), crate::method::MaxMethod>(Self::[<call_ft $i>])),
                        std::ffi::CString::new(concat!("ft", $i)).unwrap().as_ptr(),
                        max_sys::e_max_atomtypes::A_FLOAT, 0
                    );

                    max_sys::class_addmethod(class,
                        Some(std::mem::transmute::<extern "C" fn(&Self, i64), crate::method::MaxMethod>(Self::[<call_in $i>])),
                        std::ffi::CString::new(concat!("in", $i)).unwrap().as_ptr(),
                        max_sys::e_max_atomtypes::A_LONG, 0
                    );
                })*
            }
        }
    };
}

impl<O, I, T> Wrapper<O, I, T>
where
    I: WrapperInternal<O, T>,
    T: ObjWrapped<T>,
{
    fn internal(&self) -> &I {
        unsafe { &*self.wrapped.as_ptr() }
    }

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

    fn register_common<F>(
        lookup_class: bool,
        notification_handler: extern "C" fn(
            &Wrapper<O, I, T>,
            sender_name: *mut max_sys::t_symbol,
            message: *mut max_sys::t_symbol,
            sender: *mut c_void,
            data: *mut c_void,
        ),
        creator: F,
    ) where
        F: Fn() -> Class<Self>,
    {
        let key = key::<T>();
        let mut h = CLASSES.lock().expect("couldn't lock CLASSES mutex");
        if !h.contains_key(key) {
            //don't lookup class unless we want to, because max might try to register it which
            //could cause a loop
            let existing = if lookup_class {
                Class::<T>::find_in_max(T::class_name(), T::class_type())
            } else {
                std::ptr::null_mut()
            };
            let max_class = if existing.is_null() {
                let mut c = creator();
                //register notifications
                unsafe {
                    max_sys::class_addmethod(
                        c.inner(),
                        Some(std::mem::transmute::<_, MaxMethod>(notification_handler)),
                        std::ffi::CString::new("notify").unwrap().as_ptr(),
                        max_sys::e_max_atomtypes::A_CANT,
                        0,
                    );
                }
                c.register(T::class_type())
                    .expect(format!("failed to register {}", key).as_str());

                //register our ft1, ft2.. in1, in2.. tramps
                Self::register_ft_in(c.inner());
                c.inner()
            } else {
                existing
            };

            h.insert(key, ClassMaxObjWrapper(max_class));
        }
    }

    int_float_tramps!(1, 2, 3, 4, 5, 6, 7, 8, 9);
}

fn key<T>() -> &'static str {
    std::any::type_name::<T>()
}

impl<T> Wrapper<max_sys::t_object, MaxWrapperInternal<T>, T>
where
    T: MaxObjWrapped<T>,
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
    pub unsafe fn register(lookup_class: bool) {
        Self::register_common(lookup_class, Self::handle_notification_tramp, || {
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
    pub unsafe extern "C" fn new_tramp(
        sym: *mut max_sys::t_symbol,
        argc: c_long,
        argv: *const max_sys::t_atom,
    ) -> *mut c_void {
        let sym: SymbolRef = sym.into();
        let args = std::slice::from_raw_parts(std::mem::transmute::<_, _>(argv), argc as usize);
        let o = ObjBox::into_raw(Self::new(sym, &args));
        assert_eq!((&*o).max_obj(), (&*o).wrapped().max_obj());
        std::mem::transmute::<_, _>(o)
    }

    /// Create an instance of the wrapper, on the heap, with no arguments.
    pub fn new_noargs() -> ObjBox<Self> {
        Self::new(crate::max::common_symbols().s_nothing.into(), &[])
    }

    /// Create an instance of the wrapper, on the heap.
    pub fn new(sym: SymbolRef, args: &[Atom]) -> ObjBox<Self> {
        new_common(key::<T>(), |max_class| unsafe {
            let mut o: ObjBox<Self> = ObjBox::alloc(max_class);
            let internal = MaxWrapperInternal::<T>::new(o.max_obj(), sym.clone(), args);
            o.wrapped = MaybeUninit::new(internal);
            o
        })
    }

    extern "C" fn handle_notification_tramp(
        &self,
        sender_name: *mut max_sys::t_symbol,
        message: *mut max_sys::t_symbol,
        sender: *mut c_void,
        data: *mut c_void,
    ) {
        let notification = Notification::new(sender_name, message, sender, data);
        self.internal().handle_notification(&notification);
    }
}

use std::os::raw::c_long;

impl<T> MSPObjWrapper<T>
where
    T: MSPObjWrapped<T> + Sync + 'static,
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
    pub unsafe fn register(lookup_class: bool) {
        Self::register_common(lookup_class, Self::handle_notification_tramp, || {
            let mut c: Class<Self> = Class::new(
                T::class_name(),
                Self::new_tramp,
                Some(
                    std::mem::transmute::<extern "C" fn(&mut Self), MaxFree<Self>>(Self::free_msp),
                ),
            );
            //TODO somehow pass the lock so that classes can register additional classes
            MSPWrapperInternal::<T>::class_setup(&mut c);
            max_sys::class_addmethod(
                c.inner(),
                Some(std::mem::transmute::<
                    extern "C" fn(
                        &mut Self,
                        dsp64: *mut max_sys::t_object,
                        count: *mut std::os::raw::c_short,
                        samplerate: f64,
                        maxvectorsize: i64,
                        flags: i64,
                    ),
                    MaxMethod,
                >(Self::dsp64)),
                CString::new("dsp64").unwrap().as_ptr(),
                max_sys::e_max_atomtypes::A_CANT,
                0,
            );
            max_sys::class_dspinit(c.inner());
            c
        });
    }

    /// A method for Max to create an instance of your class.
    pub unsafe extern "C" fn new_tramp(
        sym: *mut max_sys::t_symbol,
        argc: c_long,
        argv: *const max_sys::t_atom,
    ) -> *mut c_void {
        let sym: SymbolRef = sym.into();
        let args = std::slice::from_raw_parts(std::mem::transmute::<_, _>(argv), argc as usize);
        let o = ObjBox::into_raw(Self::new(sym, &args));
        assert_eq!((&*o).msp_obj(), (&*o).wrapped().msp_obj());
        std::mem::transmute::<_, _>(o)
    }

    /// Create an instance of the wrapper, on the heap.
    pub fn new(sym: SymbolRef, args: &[Atom]) -> ObjBox<Self> {
        unsafe {
            new_common(key::<T>(), |max_class| {
                let mut o: ObjBox<Self> = ObjBox::alloc(max_class);
                let internal = MSPWrapperInternal::<T>::new(o.msp_obj(), sym.clone(), args);
                o.wrapped = MaybeUninit::new(internal);
                o
            })
        }
    }

    extern "C" fn free_msp(&mut self) {
        //free dsp first
        unsafe {
            max_sys::z_dsp_free(self.msp_obj());
        }
        self.free_wrapped();
    }

    extern "C" fn perform64(
        &mut self,
        dsp64: *mut max_sys::t_object,
        ins: *const *const f64,
        numins: i64,
        outs: *mut *mut f64,
        numouts: i64,
        sampleframes: i64,
        flags: i64,
        userparam: *mut c_void,
    ) {
        unsafe {
            (&mut *self.wrapped.as_mut_ptr()).perform64(
                dsp64,
                ins,
                numins,
                outs,
                numouts,
                sampleframes,
                flags,
                userparam,
            );
        }
    }

    extern "C" fn dsp64(
        &mut self,
        dsp64: *mut max_sys::t_object,
        _count: *mut std::os::raw::c_short,
        _samplerate: f64,
        _maxvectorsize: i64,
        _flags: i64,
    ) {
        unsafe {
            max_sys::dsp_add64(
                dsp64,
                self.max_obj(),
                Some(std::mem::transmute::<
                    extern "C" fn(
                        &mut Self,
                        dsp64: *mut max_sys::t_object,
                        ins: *const *const f64,
                        numins: i64,
                        outs: *mut *mut f64,
                        numouts: i64,
                        sampleframes: i64,
                        flags: i64,
                        userparam: *mut c_void,
                    ),
                    unsafe extern "C" fn(
                        x: *mut max_sys::t_object,
                        dsp64: *mut max_sys::t_object,
                        ins: *mut *mut f64,
                        numins: c_long,
                        outs: *mut *mut f64,
                        numouts: c_long,
                        sampleframes: c_long,
                        flags: c_long,
                        userparam: *mut c_void,
                    ),
                >(Self::perform64)),
                0,
                std::ptr::null_mut(),
            );
        }
    }

    extern "C" fn handle_notification_tramp(
        &self,
        sender_name: *mut max_sys::t_symbol,
        message: *mut max_sys::t_symbol,
        sender: *mut c_void,
        data: *mut c_void,
    ) {
        let notification = Notification::new(sender_name, message, sender, data);
        self.internal().handle_notification(&notification);
    }
}

impl WrappedAttachment {
    pub(crate) fn new(
        client: *mut max_sys::t_object,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Arc<Self>, ()> {
        let p = unsafe { max_sys::object_attach(namespace.inner(), name.inner(), client as _) };
        if p.is_null() {
            Err(())
        } else {
            Ok(Arc::new(WrappedAttachment {
                namespace,
                name,
                client: client as _,
            }))
        }
    }
}

impl Drop for WrappedAttachment {
    fn drop(&mut self) {
        unsafe {
            let _ =
                max_sys::object_detach(self.namespace.inner(), self.name.inner(), self.client as _);
        }
    }
}

impl WrappedAttachmentHandle {
    pub(crate) fn new(attachment: &Arc<WrappedAttachment>) -> Self {
        Self {
            inner: Arc::downgrade(attachment),
        }
    }
}

impl WrappedSubscriptionHandle {
    pub(crate) fn new(attachment: &Arc<Subscription>) -> Self {
        Self {
            inner: Arc::downgrade(attachment),
        }
    }
}

unsafe impl Send for WrappedAttachmentHandle {}
unsafe impl Send for WrappedSubscriptionHandle {}

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

unsafe impl<T> MaxObj for T
where
    T: MaxObjWrapped<T>,
{
    fn max_obj(&self) -> *mut max_sys::t_object {
        //can't seem to get from wrapper to internal because of MaybeUninit?
        let off1 = field_offset::offset_of!(Wrapper::<max_sys::t_object, MaxWrapperInternal<T>, T> => wrapped);
        let off2 = field_offset::offset_of!(MaxWrapperInternal::<T> => wrapped);
        unsafe {
            let ptr: *mut u8 = std::mem::transmute::<_, *mut u8>(self as *const T);
            std::mem::transmute::<_, *mut max_sys::t_object>(
                ptr.offset(-((off1.get_byte_offset() + off2.get_byte_offset()) as isize)),
            )
        }
    }
}

unsafe impl<T> MSPObj for T
where
    T: MSPObjWrapped<T>,
{
    fn msp_obj(&self) -> *mut max_sys::t_pxobject {
        //can't seem to get from wrapper to internal because of MaybeUninit?
        let off1 = field_offset::offset_of!(Wrapper::<max_sys::t_pxobject, MSPWrapperInternal<T>, T> => wrapped);
        let off2 = field_offset::offset_of!(MSPWrapperInternal::<T> => wrapped);
        unsafe {
            let ptr: *mut u8 = std::mem::transmute::<_, *mut u8>(self as *const T);
            std::mem::transmute::<_, *mut max_sys::t_pxobject>(
                ptr.offset(-((off1.get_byte_offset() + off2.get_byte_offset()) as isize)),
            )
        }
    }
}

impl<T> WrappedDefer<MaxObjWrapper<T>> for T
where
    T: MaxObjWrapped<T>,
{
    fn defer(&self, meth: DeferMethodWrapped<MaxObjWrapper<T>>, sym: SymbolRef, atoms: &[Atom]) {
        let obj = self.max_obj();
        crate::thread::defer(
            unsafe { std::mem::transmute::<_, _>(meth) },
            obj,
            sym,
            atoms,
        );
    }

    fn defer_low(
        &self,
        meth: DeferMethodWrapped<MaxObjWrapper<T>>,
        sym: SymbolRef,
        atoms: &[Atom],
    ) {
        let obj = self.max_obj();
        crate::thread::defer_low(
            unsafe { std::mem::transmute::<_, _>(meth) },
            obj,
            sym,
            atoms,
        );
    }
}

impl<T> WrappedDefer<MSPObjWrapper<T>> for T
where
    T: MSPObjWrapped<T>,
{
    fn defer(&self, meth: DeferMethodWrapped<MSPObjWrapper<T>>, sym: SymbolRef, atoms: &[Atom]) {
        let obj = self.as_max_obj();
        crate::thread::defer(
            unsafe { std::mem::transmute::<_, _>(meth) },
            obj,
            sym,
            atoms,
        );
    }

    fn defer_low(
        &self,
        meth: DeferMethodWrapped<MSPObjWrapper<T>>,
        sym: SymbolRef,
        atoms: &[Atom],
    ) {
        let obj = self.as_max_obj();
        crate::thread::defer_low(
            unsafe { std::mem::transmute::<_, _>(meth) },
            obj,
            sym,
            atoms,
        );
    }
}

impl<T> WrappedAttach<MaxObjWrapper<T>> for T
where
    T: MaxObjWrapped<T>,
{
    fn attach(&self, namespace: SymbolRef, name: SymbolRef) -> Result<WrappedAttachmentHandle, ()> {
        let wrapper: &MaxObjWrapper<T> = unsafe { std::mem::transmute::<_, _>(self.max_obj()) };
        wrapper.internal().attach(self.max_obj(), namespace, name)
    }

    fn detach(&self, handle: WrappedAttachmentHandle) {
        let wrapper: &MaxObjWrapper<T> = unsafe { std::mem::transmute::<_, _>(self.max_obj()) };
        wrapper.internal().detatch(handle)
    }

    fn subscribe(
        &self,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> WrappedSubscriptionHandle {
        let wrapper: &MaxObjWrapper<T> = unsafe { std::mem::transmute::<_, _>(self.max_obj()) };
        wrapper
            .internal()
            .subscribe(self.max_obj(), namespace, name, class_name)
    }

    ///unsubscribe from the subscription.
    fn unsubscribe(&self, handle: WrappedSubscriptionHandle) {
        let wrapper: &MaxObjWrapper<T> = unsafe { std::mem::transmute::<_, _>(self.max_obj()) };
        wrapper.internal().unsubscribe(handle);
    }
}

impl<T> WrappedAttach<MSPObjWrapper<T>> for T
where
    T: MSPObjWrapped<T>,
{
    fn attach(&self, namespace: SymbolRef, name: SymbolRef) -> Result<WrappedAttachmentHandle, ()> {
        let wrapper: &MSPObjWrapper<T> = unsafe { std::mem::transmute::<_, _>(self.as_max_obj()) };
        wrapper
            .internal()
            .attach(self.as_max_obj(), namespace, name)
    }

    fn detach(&self, handle: WrappedAttachmentHandle) {
        let wrapper: &MSPObjWrapper<T> = unsafe { std::mem::transmute::<_, _>(self.as_max_obj()) };
        wrapper.internal().detatch(handle)
    }

    fn subscribe(
        &self,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> WrappedSubscriptionHandle {
        let wrapper: &MSPObjWrapper<T> = unsafe { std::mem::transmute::<_, _>(self.as_max_obj()) };
        wrapper
            .internal()
            .subscribe(self.as_max_obj(), namespace, name, class_name)
    }

    ///unsubscribe from the subscription.
    fn unsubscribe(&self, handle: WrappedSubscriptionHandle) {
        let wrapper: &MSPObjWrapper<T> = unsafe { std::mem::transmute::<_, _>(self.as_max_obj()) };
        wrapper.internal().unsubscribe(handle);
    }
}
