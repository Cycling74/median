//! Utilities for building objects.
use crate::{
    atom::Atom,
    clock::ClockHandle,
    inlet::{MSPInlet, MaxInlet, Proxy},
    outlet::{OutAnything, OutBang, OutFloat, OutInt, OutList, Outlet},
    symbol::SymbolRef,
    wrapper::{
        FloatCBHash, IntCBHash, MSPObjWrapped, MSPObjWrapper, MaxObjWrapped, MaxObjWrapper,
        ObjWrapped, WrapperWrapped,
    },
};
use std::collections::HashMap;
use std::marker::PhantomData;

pub struct WrappedBuilder<'a, T, W> {
    max_obj: *mut max_sys::t_object,
    msp_obj: Option<*mut max_sys::t_pxobject>,
    sym: SymbolRef,
    args: &'a [Atom],
    inlets: Vec<MSPInlet<T>>, //just use MSP since it contains all of Max
    signal_outlets: usize,
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

    /// Get the arguments that were passed to this object on creation.
    fn creation_args(&self) -> &[Atom];

    /// Get the symbol that were passed used when creating this object.
    fn creation_symbol(&self) -> SymbolRef;

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

impl<'a, T, W> WrappedBuilder<'a, T, W> {
    pub fn new_max(owner: *mut max_sys::t_object, sym: SymbolRef, args: &'a [Atom]) -> Self {
        Self {
            max_obj: owner,
            msp_obj: None,
            sym,
            args,
            inlets: Vec::new(),
            signal_outlets: 0,
            _phantom: PhantomData,
        }
    }

    pub fn new_msp(owner: *mut max_sys::t_pxobject, sym: SymbolRef, args: &'a [Atom]) -> Self {
        Self {
            max_obj: owner as _,
            msp_obj: Some(owner),
            sym,
            args,
            inlets: Vec::new(),
            signal_outlets: 0,
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
            self.signal_outlets,
        )
    }

    // pass None for Max objects
    fn finalize_inlets(
        &mut self,
        signal_inlets: Option<usize>,
    ) -> (FloatCBHash<T>, IntCBHash<T>, Vec<Proxy>) {
        let mut callbacks_float = HashMap::new();
        let mut callbacks_int = HashMap::new();
        let mut proxies = Vec::new();

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
                MSPInlet::Float(cb) => unsafe {
                    assert!(index > 0 && index < 10, "index out of range");
                    let _ = max_sys::floatin(self.max_obj as _, index as _);
                    let _ = callbacks_float.insert(index, cb);
                },
                MSPInlet::Int(cb) => unsafe {
                    assert!(index > 0 && index < 10, "index out of range");
                    let _ = max_sys::intin(self.max_obj as _, index as _);
                    let _ = callbacks_int.insert(index, cb);
                },
                MSPInlet::Proxy => {
                    proxies.push(Proxy::new(self.max_obj, index));
                }
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

        (callbacks_float, callbacks_int, proxies)
    }
}

impl<'a, T, W> ObjBuilder<T> for WrappedBuilder<'a, T, W>
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
        Outlet::append_bang(self.max_obj)
    }
    /// Add an outlet that outputs floats.
    fn add_float_outlet(&mut self) -> OutFloat {
        Outlet::append_float(self.max_obj)
    }
    /// Add an outlet that outputs ints.
    fn add_int_outlet(&mut self) -> OutInt {
        Outlet::append_int(self.max_obj)
    }
    /// Add an outlet that outputs lists.
    fn add_list_outlet(&mut self) -> OutList {
        Outlet::append_list(self.max_obj)
    }
    /// Add an outlet that outputs anything Max supports.
    fn add_anything_outlet(&mut self) -> OutAnything {
        Outlet::append_anything(self.max_obj)
    }
    fn creation_args(&self) -> &[Atom] {
        self.args
    }
    fn creation_symbol(&self) -> SymbolRef {
        self.sym.clone()
    }

    /// Get the Max object for the wrapper of this object.
    unsafe fn max_obj(&mut self) -> *mut max_sys::t_object {
        return self.max_obj;
    }
}

impl<'a, T> MaxWrappedBuilder<T> for WrappedBuilder<'a, T, MaxObjWrapper<T>>
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

impl<'a, T> MSPWrappedBuilder<T> for WrappedBuilder<'a, T, MSPObjWrapper<T>>
where
    T: MSPObjWrapped<T>,
{
    /// Add signal outlets
    fn add_signal_outlets(&mut self, count: usize) {
        for _ in 0..count {
            Outlet::append_signal(self.max_obj);
        }
        self.signal_outlets += count;
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
    pub proxy_inlets: Vec<Proxy>,
}

pub struct MSPWrappedBuilderFinalize<T> {
    pub signal_inlets: usize,
    pub signal_outlets: usize,
    pub callbacks_float: FloatCBHash<T>,
    pub callbacks_int: IntCBHash<T>,
    pub proxy_inlets: Vec<Proxy>,
}

impl<'a, T> WrappedBuilder<'a, T, MaxObjWrapper<T>>
where
    T: MaxObjWrapped<T>,
{
    pub fn finalize(mut self) -> MaxWrappedBuilderFinalize<T> {
        let (callbacks_float, callbacks_int, proxy_inlets) = self.finalize_inlets(None);
        MaxWrappedBuilderFinalize {
            callbacks_float,
            callbacks_int,
            proxy_inlets,
        }
    }
}

impl<'a, T> WrappedBuilder<'a, T, MSPObjWrapper<T>>
where
    T: MSPObjWrapped<T>,
{
    //finalize and return siginal (inlets, outlets) counts
    pub fn finalize(mut self) -> MSPWrappedBuilderFinalize<T> {
        let (signal_inlets, signal_outlets) = self.signal_iolets();
        let (callbacks_float, callbacks_int, proxy_inlets) =
            self.finalize_inlets(Some(signal_inlets));
        MSPWrappedBuilderFinalize {
            signal_inlets,
            signal_outlets,
            callbacks_float,
            callbacks_int,
            proxy_inlets,
        }
    }
}
