use crate::{
    clock::ClockHandle,
    wrapper::{MaxObjWrapped, MaxObjWrapper, ObjWrapped, WrapperWrapped},
};
use std::marker::PhantomData;

pub type MSPWrappedBuilderInitial<T, W> = MSPWrappedBuilder<T, W, MSPSetupState>;
pub type MSPWrappedBuilderFinal<T, W> = MSPWrappedBuilder<T, W, MSPWithInputsState>;

pub struct MaxWrappedBuilder<T> {
    owner: *mut max_sys::t_object,
    _phantom: PhantomData<T>,
}

pub trait MSPBuilderState {}
pub struct MSPSetupState {}
pub struct MSPWithInputsState {}

impl MSPBuilderState for MSPSetupState {}
impl MSPBuilderState for MSPWithInputsState {}

pub struct MSPWrappedBuilder<T, W, S> {
    owner: *mut max_sys::t_pxobject,
    ins: usize,
    outs: usize,
    _phantom: PhantomData<(T, W, S)>,
}

impl<T> MaxWrappedBuilder<T>
where
    T: MaxObjWrapped<T> + Send + Sync + 'static,
{
    pub fn new(owner: *mut max_sys::t_object) -> Self {
        Self {
            owner,
            _phantom: PhantomData,
        }
    }
    /// Create a clock with a method callback
    pub fn with_clockfn(&mut self, func: fn(&T)) -> ClockHandle {
        clockfn::<T, MaxObjWrapper<T>>(self.owner, func)
    }
    /// Create a clock with a closure callback
    pub fn with_clock(&mut self, func: Box<dyn Fn(&T)>) -> ClockHandle {
        clock::<T, MaxObjWrapper<T>>(self.owner, func)
    }
    /// Get the owner object.
    pub unsafe fn max_obj(&mut self) -> *mut max_sys::t_object {
        self.owner
    }
}

impl<T, W, S> MSPWrappedBuilder<T, W, S>
where
    T: ObjWrapped<T> + Send + Sync + 'static,
    W: WrapperWrapped<T>,
    S: MSPBuilderState,
{
    /// Get the owner object.
    pub unsafe fn msp_obj(&mut self) -> *mut max_sys::t_pxobject {
        self.owner
    }

    pub unsafe fn max_obj(&mut self) -> *mut max_sys::t_object {
        std::mem::transmute::<_, _>(self.owner)
    }

    pub fn ins(&self) -> usize {
        self.ins
    }

    pub fn outs(&self) -> usize {
        self.outs
    }
}

impl<T, W> MSPWrappedBuilder<T, W, MSPSetupState>
where
    T: ObjWrapped<T> + Send + Sync + 'static,
    W: WrapperWrapped<T>,
{
    /// Create a builder for setting up wrapped MSP objects.
    pub fn new(owner: *mut max_sys::t_pxobject) -> Self {
        Self {
            owner,
            ins: 0,
            outs: 0,
            _phantom: PhantomData,
        }
    }

    /// Specify the number of inputs then continue setup.
    pub fn with_inputs(mut self, ins: usize) -> MSPWrappedBuilder<T, W, MSPWithInputsState> {
        unsafe {
            max_sys::z_dsp_setup(self.msp_obj(), ins as _);
        }
        MSPWrappedBuilder {
            ins,
            outs: self.outs,
            owner: self.owner,
            _phantom: PhantomData,
        }
    }
}

impl<T, W> MSPWrappedBuilder<T, W, MSPWithInputsState>
where
    T: ObjWrapped<T> + Send + Sync + 'static,
    W: WrapperWrapped<T>,
{
    /// Create a clock with a method callback
    pub fn with_clockfn(&mut self, func: fn(&T)) -> ClockHandle {
        clockfn::<T, W>(unsafe { self.max_obj() }, func)
    }
    /// Create a clock with a closure callback
    pub fn with_clock(&mut self, func: Box<dyn Fn(&T)>) -> ClockHandle {
        clock::<T, W>(unsafe { self.max_obj() }, func)
    }

    pub fn add_signal_outlet(&mut self) {
        self.add_signal_outlets(1);
    }
    pub fn add_signal_outlets(&mut self, count: usize) {
        let signal = std::ffi::CString::new("signal").expect("failed to create cstring");
        for _ in 0..count {
            unsafe {
                max_sys::outlet_new(self.max_obj() as _, signal.as_ptr());
            }
        }
        self.outs += count;
    }
}

fn clockfn<T, W>(owner: *mut max_sys::t_object, func: fn(&T)) -> ClockHandle
where
    T: ObjWrapped<T> + Send + Sync + 'static,
    W: WrapperWrapped<T>,
{
    unsafe {
        ClockHandle::new(
            // XXX wrapper should outlive the ClockHandle, but we haven't guaranteed that..
            owner,
            Box::new(move |wrapper| {
                let wrapper: &W = std::mem::transmute::<_, &W>(wrapper);
                func(wrapper.wrapped());
            }),
        )
    }
}

fn clock<T, W>(owner: *mut max_sys::t_object, func: Box<dyn Fn(&T)>) -> ClockHandle
where
    T: ObjWrapped<T> + Send + Sync + 'static,
    W: WrapperWrapped<T>,
{
    unsafe {
        ClockHandle::new(
            // XXX wrapper should outlive the ClockHandle, but we haven't guaranteed that..
            owner,
            Box::new(move |wrapper| {
                let wrapper: &W = std::mem::transmute::<_, &W>(wrapper);
                func(wrapper.wrapped());
            }),
        )
    }
}
