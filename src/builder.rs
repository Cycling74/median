use crate::{
    clock::ClockHandle,
    inlet::{MSPInlet, MaxInlet},
    outlet::{OutAnything, OutBang, OutFloat, OutInt, OutList, Outlet},
    wrapper::{
        FloatCBHash, IntCBHash, MSPObjWrapped, MSPObjWrapper, MaxObjWrapped, MaxObjWrapper,
        ObjWrapped, WrapperWrapped,
    },
};
use std::collections::HashMap;
use std::marker::PhantomData;

//hold outlets until after they've all been allocated, then init
enum UninitOutlet {
    Bang(*mut Outlet),
    Float(*mut Outlet),
    Int(*mut Outlet),
    List(*mut Outlet),
    Anything(*mut Outlet),
    Signal,
}

pub struct WrappedBuilder<T, W> {
    max_obj: *mut max_sys::t_object,
    msp_obj: Option<*mut max_sys::t_pxobject>,
    inlets: Vec<MSPInlet<T>>, //just use MSP since it contains all of Max
    outlets: Vec<UninitOutlet>,
    _phantom: PhantomData<(T, W)>,
}

/// Builder for your object
///
/// # Remarks
/// Unlike the Max SDK, inlets and outlets are specified from left to right.
pub trait ObjBuilder<T> {
    fn with_clockfn(&mut self, func: fn(&T)) -> ClockHandle;
    fn with_clock(&mut self, func: Box<dyn Fn(&T)>) -> ClockHandle;
    /// Add an outlet that outputs bangs.
    fn add_bang_outlet(&mut self) -> OutBang;
    /// Add an outlet that outputs floats.
    fn add_float_outlet(&mut self) -> OutFloat;
    /// Add an outlet that outputs ints.
    fn add_int_outlet(&mut self) -> OutInt;
    /// Add an outlet that outputs lists.
    fn add_list_outlet(&mut self) -> OutList;
    /// Add an outlet that outputs anything Max supports.
    fn add_anything_outlet(&mut self) -> OutAnything;

    /// Get the Max object for the wrapper of this object.
    unsafe fn max_obj(&mut self) -> *mut max_sys::t_object;
}

pub trait MaxWrappedBuilder<T>: ObjBuilder<T> {
    /// Add an inlet, left to right, returns index.
    fn add_inlet(&mut self, inlet_type: MaxInlet<T>) -> usize;
}

pub trait MSPWrappedBuilder<T>: ObjBuilder<T> {
    /// Add signal outlets
    fn add_signal_outlets(&mut self, count: usize);

    /// Add signal inlets.
    /// # Panics
    /// Will panic if called more than once.
    fn add_signal_inlets(&mut self, count: usize);

    /// Add an inlet, left to right, returns index.
    fn add_inlet(&mut self, inlet_type: MSPInlet<T>) -> usize;

    /// Get the MSP object for the wrapper of this object.
    unsafe fn msp_obj(&mut self) -> *mut max_sys::t_pxobject;
}

impl<T, W> WrappedBuilder<T, W> {
    pub fn new_max(owner: *mut max_sys::t_object) -> Self {
        Self {
            max_obj: owner,
            msp_obj: None,
            inlets: Vec::new(),
            outlets: Vec::new(),
            _phantom: PhantomData,
        }
    }

    pub fn new_msp(owner: *mut max_sys::t_pxobject) -> Self {
        Self {
            max_obj: owner as _,
            msp_obj: Some(owner),
            inlets: Vec::new(),
            outlets: Vec::new(),
            _phantom: PhantomData,
        }
    }

    //get the number of (inlets, outlets)
    fn signal_iolets(&self) -> (usize, usize) {
        (
            self.inlets
                .iter()
                .filter(|i| match i {
                    MSPInlet::Signal => true,
                    _ => false,
                })
                .count(),
            self.outlets
                .iter()
                .filter(|i| match i {
                    UninitOutlet::Signal => true,
                    _ => false,
                })
                .count(),
        )
    }

    // pass None for Max objects
    fn finalize_inlets(&mut self, signal_inlets: Option<usize>) -> (FloatCBHash<T>, IntCBHash<T>) {
        let mut callbacks_float = HashMap::new();
        let mut callbacks_int = HashMap::new();

        //handle an MSP object with 0 signal inlets
        let mut called_dsp_setup = match signal_inlets {
            Some(0) => unsafe {
                max_sys::z_dsp_setup(self.msp_obj.expect("need to have msp object for dsp"), 0);
                true
            },
            _ => false,
        };

        let inlets = std::mem::take(&mut self.inlets);

        //reverse because max allocs inlets right to left
        for (mut index, inlet) in inlets.into_iter().enumerate().rev() {
            index = index + 1; //TODO allow for no default inlet
            match inlet {
                MSPInlet::Float(cb) => {
                    let _ = callbacks_float.insert(index, cb);
                }
                MSPInlet::Int(cb) => {
                    let _ = callbacks_int.insert(index, cb);
                }
                MSPInlet::Proxy => panic!("proxy not supported yet"),
                MSPInlet::Signal => {
                    if !called_dsp_setup {
                        called_dsp_setup = true;
                        unsafe {
                            max_sys::z_dsp_setup(
                                self.msp_obj.expect("need to have msp object for dsp"),
                                signal_inlets.expect("should have signal inlets") as _,
                            );
                        }
                    }
                }
            };
        }

        (callbacks_float, callbacks_int)
    }

    fn finalize_outlets(&mut self) {
        let signal = std::ffi::CString::new("signal").expect("failed to create cstring");
        //allocate in reverse, another box exists so leak it
        //TODO if the caller drops the box then this will panic, maybe just use Arc?
        for outlet in self.outlets.iter_mut().rev() {
            match outlet {
                UninitOutlet::Bang(o) => unsafe {
                    let mut o = Box::from_raw(*o);
                    o.init_bang(self.max_obj);
                    Box::leak(o);
                },
                UninitOutlet::Float(o) => unsafe {
                    let mut o = Box::from_raw(*o);
                    o.init_float(self.max_obj);
                    Box::leak(o);
                },
                UninitOutlet::Int(o) => unsafe {
                    let mut o = Box::from_raw(*o);
                    o.init_int(self.max_obj);
                    Box::leak(o);
                },
                UninitOutlet::List(o) => unsafe {
                    let mut o = Box::from_raw(*o);
                    o.init_list(self.max_obj);
                    Box::leak(o);
                },
                UninitOutlet::Anything(o) => unsafe {
                    let mut o = Box::from_raw(*o);
                    o.init_anything(self.max_obj);
                    Box::leak(o);
                },
                UninitOutlet::Signal => unsafe {
                    max_sys::outlet_new(self.max_obj as _, signal.as_ptr());
                },
            }
        }
    }
}

impl<T, W> ObjBuilder<T> for WrappedBuilder<T, W>
where
    T: ObjWrapped<T>,
    W: WrapperWrapped<T>,
{
    fn with_clockfn(&mut self, func: fn(&T)) -> ClockHandle {
        unsafe {
            ClockHandle::new(
                // XXX wrapper should outlive the ClockHandle, but we haven't guaranteed that..
                self.max_obj(),
                Box::new(move |wrapper| {
                    let wrapper: &W = std::mem::transmute::<_, &W>(wrapper);
                    func(wrapper.wrapped());
                }),
            )
        }
    }
    fn with_clock(&mut self, func: Box<dyn Fn(&T)>) -> ClockHandle {
        unsafe {
            ClockHandle::new(
                // XXX wrapper should outlive the ClockHandle, but we haven't guaranteed that..
                self.max_obj(),
                Box::new(move |wrapper| {
                    let wrapper: &W = std::mem::transmute::<_, &W>(wrapper);
                    func(wrapper.wrapped());
                }),
            )
        }
    }
    /// Add an outlet that outputs bangs.
    fn add_bang_outlet(&mut self) -> OutBang {
        let mut b = Outlet::new_null();
        self.outlets.push(UninitOutlet::Bang(b.as_mut()));
        b as _
    }
    /// Add an outlet that outputs floats.
    fn add_float_outlet(&mut self) -> OutFloat {
        let mut b = Outlet::new_null();
        self.outlets.push(UninitOutlet::Float(b.as_mut()));
        b as _
    }
    /// Add an outlet that outputs ints.
    fn add_int_outlet(&mut self) -> OutInt {
        let mut b = Outlet::new_null();
        self.outlets.push(UninitOutlet::Int(b.as_mut()));
        b as _
    }
    /// Add an outlet that outputs lists.
    fn add_list_outlet(&mut self) -> OutList {
        let mut b = Outlet::new_null();
        self.outlets.push(UninitOutlet::List(b.as_mut()));
        b as _
    }
    /// Add an outlet that outputs anything Max supports.
    fn add_anything_outlet(&mut self) -> OutAnything {
        let mut b = Outlet::new_null();
        self.outlets.push(UninitOutlet::Anything(b.as_mut()));
        b as _
    }
    /// Get the Max object for the wrapper of this object.
    unsafe fn max_obj(&mut self) -> *mut max_sys::t_object {
        return self.max_obj;
    }
}

impl<T> MaxWrappedBuilder<T> for WrappedBuilder<T, MaxObjWrapper<T>>
where
    T: MaxObjWrapped<T>,
{
    /// Add an inlet, left to right, returns index.
    fn add_inlet(&mut self, inlet_type: MaxInlet<T>) -> usize {
        //convert to MSP so we can share the builder
        self.inlets.push(match inlet_type {
            MaxInlet::Int(f) => MSPInlet::Int(f),
            MaxInlet::Float(f) => MSPInlet::Float(f),
            MaxInlet::Proxy => MSPInlet::Proxy,
        });
        self.inlets.len() //there is a default inlet that we don't specify
    }
}

impl<T> MSPWrappedBuilder<T> for WrappedBuilder<T, MSPObjWrapper<T>>
where
    T: MSPObjWrapped<T>,
{
    /// Add signal outlets
    fn add_signal_outlets(&mut self, count: usize) {
        for _ in 0..count {
            self.outlets.push(UninitOutlet::Signal);
        }
    }

    /// Add signal inlets.
    /// # Panics
    /// Will panic if called more than once.
    fn add_signal_inlets(&mut self, count: usize) {
        //make sure we don't have any signals already
        assert!(
            !self.inlets.iter().any(|i| match i {
                MSPInlet::Signal => true,
                _ => false,
            }),
            "can only specify signal inlets once"
        );
        for _ in 0..count {
            self.inlets.push(MSPInlet::Signal);
        }
    }

    /// Add an inlet, left to right, returns index.
    fn add_inlet(&mut self, inlet_type: MSPInlet<T>) -> usize {
        self.inlets.push(inlet_type);
        self.inlets.len() //there is a default inlet that we don't specify
    }

    /// Get the MSP object for the wrapper of this object.
    unsafe fn msp_obj(&mut self) -> *mut max_sys::t_pxobject {
        self.msp_obj.expect("expected to have msp_obj")
    }
}

pub struct MaxWrappedBuilderFinalize<T> {
    pub callbacks_float: FloatCBHash<T>,
    pub callbacks_int: IntCBHash<T>,
}

pub struct MSPWrappedBuilderFinalize<T> {
    pub signal_inlets: usize,
    pub signal_outlets: usize,
    pub callbacks_float: FloatCBHash<T>,
    pub callbacks_int: IntCBHash<T>,
}

impl<T> WrappedBuilder<T, MaxObjWrapper<T>>
where
    T: MaxObjWrapped<T>,
{
    pub fn finalize(mut self) -> MaxWrappedBuilderFinalize<T> {
        self.finalize_outlets();
        let (callbacks_float, callbacks_int) = self.finalize_inlets(None);
        MaxWrappedBuilderFinalize {
            callbacks_float,
            callbacks_int,
        }
    }
}

impl<T> WrappedBuilder<T, MSPObjWrapper<T>>
where
    T: MSPObjWrapped<T>,
{
    //finalize and return siginal (inlets, outlets) counts
    pub fn finalize(mut self) -> MSPWrappedBuilderFinalize<T> {
        let (signal_inlets, signal_outlets) = self.signal_iolets();
        self.finalize_outlets();
        let (callbacks_float, callbacks_int) = self.finalize_inlets(Some(signal_inlets));
        MSPWrappedBuilderFinalize {
            signal_inlets,
            signal_outlets,
            callbacks_float,
            callbacks_int,
        }
    }
}
