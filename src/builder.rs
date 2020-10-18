use crate::{
    clock::ClockHandle,
    object::{MSPObj, MaxObj},
    wrapper::{MSPObjWrapped, ObjWrapped, ObjWrapper},
};
use std::marker::PhantomData;

pub trait MaxWrappedBuilder<T> {
    /// Create a clock with a method callback
    fn with_clockfn(&mut self, func: fn(&T)) -> ClockHandle;
    /// Create a clock with a closure callback
    fn with_clock(&mut self, func: Box<dyn Fn(&T)>) -> ClockHandle;

    unsafe fn max_obj(&mut self) -> *mut max_sys::t_object;
}

pub trait MSPWrappedBuilder<T>: MaxWrappedBuilder<T> {
    unsafe fn msp_obj(&mut self) -> *mut max_sys::t_pxobject;
}

pub struct WrappedBuilder<'a, W, T> {
    wrapper: &'a mut W,
    _phantom: PhantomData<T>,
}

impl<'a, W, T> WrappedBuilder<'a, W, T> {
    pub fn new(wrapper: &'a mut W) -> Self {
        Self {
            wrapper,
            _phantom: PhantomData,
        }
    }
}

impl<'a, W, T> MaxWrappedBuilder<T> for WrappedBuilder<'a, W, T>
where
    W: MaxObj,
    T: ObjWrapped<T> + Send + Sync + 'static,
{
    /// Create a clock with a method callback
    fn with_clockfn(&mut self, func: fn(&T)) -> ClockHandle {
        unsafe {
            ClockHandle::new(
                // XXX wrapper should outlive the ClockHandle, but we haven't guaranteed that..
                self.wrapper.max_obj(),
                Box::new(move |wrapper| {
                    let wrapper: &ObjWrapper<max_sys::t_object, T> =
                        std::mem::transmute::<_, &ObjWrapper<max_sys::t_object, T>>(wrapper);
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
                self.wrapper.max_obj(),
                Box::new(move |wrapper| {
                    let wrapper: &ObjWrapper<max_sys::t_object, T> =
                        std::mem::transmute::<_, &ObjWrapper<max_sys::t_object, T>>(wrapper);
                    func(wrapper.wrapped());
                }),
            )
        }
    }

    /// Get the parent max object which can be cast to `&MaxObjWrapper<T>`.
    /// This in turn can be used to get your object with the `wrapped()` method.
    unsafe fn max_obj(&mut self) -> *mut max_sys::t_object {
        std::mem::transmute::<_, _>(self.wrapper.max_obj())
    }
}

impl<'a, W, T> MSPWrappedBuilder<T> for WrappedBuilder<'a, W, T>
where
    W: MSPObj,
    T: MSPObjWrapped<T> + Send + Sync + 'static,
{
    /// Get the parent msp object which can be cast to `&MSPObjWrapper<T>`.
    /// This in turn can be used to get your object with the `wrapped()` method.
    unsafe fn msp_obj(&mut self) -> *mut max_sys::t_pxobject {
        std::mem::transmute::<_, _>(self.wrapper.msp_obj())
    }
}
