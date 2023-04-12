//! Utilities for building objects.
use crate::{
    atom::Atom,
    buffer::{BufferRef, BufferReference},
    clock::ClockHandle,
    inlet::{MSPInlet, MaxInlet, Proxy},
    notify::{Attachment, AttachmentError, Registration, RegistrationError, Subscription},
    outlet::{OutAnything, OutBang, OutFloat, OutInt, OutList, Outlet},
    symbol::SymbolRef,
    wrapper::{
        FloatCBHash, IntCBHash, MSPObjWrapped, MSPObjWrapper, MaxObjWrapped, MaxObjWrapper,
        ObjWrapped, WrapperWrapped,
    },
};
use std::{collections::HashMap, ffi::CString, marker::PhantomData, sync::Arc};

pub struct WrappedBuilder<'a, T, W> {
    max_obj: *mut max_sys::t_object,
    msp_obj: Option<*mut max_sys::t_pxobject>,
    sym: SymbolRef,
    args: &'a [Atom],
    inlets: Vec<MSPInlet<T>>, //just use MSP since it contains all of Max
    buffer_refs: Vec<ManagedBufferRefInternal>,
    assist_ins: HashMap<usize, CString>,
    assist_outs: HashMap<usize, CString>,
    outlet_count: usize,
    signal_outlets: usize,
    _phantom: PhantomData<(T, W)>,
}

pub type ManagedBufferRef = Arc<dyn BufferReference>;
pub(crate) type ManagedBufferRefInternal = Arc<BufferRef>;

/// Builder for your object
///
/// # Remarks
/// Unlike the Max SDK, inlets and outlets are specified from left to right.
pub trait ObjBuilder<T> {
    /// Add assistance for the default inlet
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn with_default_inlet_assist(&mut self, assist: &str);

    /// Get a clock object that executes `func` when triggered.
    fn with_clockfn(&mut self, func: fn(&T)) -> ClockHandle;
    /// Get a clock object that executes `func` when triggered.
    fn with_clock(&mut self, func: Box<dyn Fn(&T)>) -> ClockHandle;
    /// Get a managed buffer reference.
    fn with_buffer(&mut self, name: Option<SymbolRef>) -> ManagedBufferRef;

    /// Add an outlet that outputs bangs.
    fn add_bang_outlet(&mut self) -> OutBang;

    /// Add an outlet that outputs bangs, provide assist.
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_bang_outlet_with_assist(&mut self, assist: &str) -> OutBang;

    /// Add an outlet that outputs floats.
    fn add_float_outlet(&mut self) -> OutFloat;

    /// Add an outlet that outputs floats, provide assist.
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_float_outlet_with_assist(&mut self, assist: &str) -> OutFloat;

    /// Add an outlet that outputs ints.
    fn add_int_outlet(&mut self) -> OutInt;

    /// Add an outlet that outputs ints, provide assist.
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_int_outlet_with_assist(&mut self, assist: &str) -> OutInt;

    /// Add an outlet that outputs lists.
    fn add_list_outlet(&mut self) -> OutList;

    /// Add an outlet that outputs lists, provide assist.
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_list_outlet_with_assist(&mut self, assist: &str) -> OutList;

    /// Add an outlet that outputs anything Max supports.
    fn add_anything_outlet(&mut self) -> OutAnything;

    /// Add an outlet that outputs anything Max supports, provide assist.
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_anything_outlet_with_assist(&mut self, assist: &str) -> OutAnything;

    /// Get the arguments that were passed to this object on creation.
    fn creation_args(&self) -> &[Atom];

    /// Get the symbol that were passed used when creating this object.
    fn creation_symbol(&self) -> SymbolRef;

    /// Try to your object in the given namespace with the given name;
    fn try_register(
        &self,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Registration, RegistrationError>;

    /// Attach to an object with the given name and namespace;
    fn attach(
        &mut self,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Attachment, AttachmentError>;

    ///subscribe to attach to an object with the given name in the given namespace.
    fn subscribe(
        &mut self,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> Subscription;

    /// Get the Max object for the wrapper of this object.
    unsafe fn max_obj(&mut self) -> *mut max_sys::t_object;
}

pub trait MaxWrappedBuilder<T>: ObjBuilder<T> {
    /// Add an inlet, left to right, returns index.
    fn add_inlet(&mut self, inlet_type: MaxInlet<T>) -> usize;

    /// Add an inlet with assist, left to right, returns index.
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_inlet_with_assist(&mut self, inlet_type: MaxInlet<T>, assist: &str) -> usize;
}

pub trait MSPWrappedBuilder<T>: ObjBuilder<T> {
    /// Add signal outlets
    fn add_signal_outlets(&mut self, count: usize);

    /// Add signal outlets with assistance
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_signal_outlets_with_assist(&mut self, assist: &[&str]);

    /// Add signal inlets.
    /// # Panics
    /// * Will panic if called more than once.
    fn add_signal_inlets(&mut self, count: usize);

    /// Add signal inlets with assistance.
    /// # Panics
    /// * Will panic if called more than once.
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_signal_inlets_with_assist(&mut self, assist: &[&str]);

    /// Add an inlet, left to right, returns index.
    fn add_inlet(&mut self, inlet_type: MSPInlet<T>) -> usize;

    /// Add an inlet with assist, left to right, returns index.
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_inlet_with_assist(&mut self, inlet_type: MSPInlet<T>, assist: &str) -> usize;

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
            buffer_refs: Vec::new(),
            signal_outlets: 0,
            outlet_count: 0,
            _phantom: PhantomData,
            assist_ins: HashMap::new(),
            assist_outs: HashMap::new(),
        }
    }

    pub fn new_msp(owner: *mut max_sys::t_pxobject, sym: SymbolRef, args: &'a [Atom]) -> Self {
        Self {
            max_obj: owner as _,
            msp_obj: Some(owner),
            sym,
            args,
            inlets: Vec::new(),
            buffer_refs: Vec::new(),
            signal_outlets: 0,
            outlet_count: 0,
            _phantom: PhantomData,
            assist_ins: HashMap::new(),
            assist_outs: HashMap::new(),
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

    fn add_out_assist(&mut self, i: usize, s: &str) {
        self.assist_outs.insert(
            i,
            CString::new(s).expect("to create CString from assist &str"),
        );
    }

    fn add_in_assist(&mut self, i: usize, s: &str) {
        self.assist_ins.insert(
            i,
            CString::new(s).expect("to create CString from assist &str"),
        );
    }
}

impl<'a, T, W> ObjBuilder<T> for WrappedBuilder<'a, T, W>
where
    T: ObjWrapped<T>,
    W: WrapperWrapped<T>,
{
    fn with_default_inlet_assist(&mut self, assist: &str) {
        self.add_in_assist(0, assist);
    }

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
    fn with_buffer(&mut self, name: Option<SymbolRef>) -> ManagedBufferRef {
        let b = Arc::new(unsafe { BufferRef::new(self.max_obj, name) });
        self.buffer_refs.push(b.clone());
        b
    }

    /// Add an outlet that outputs bangs.
    fn add_bang_outlet(&mut self) -> OutBang {
        self.outlet_count += 1;
        Outlet::append_bang(self.max_obj)
    }

    fn add_bang_outlet_with_assist(&mut self, assist: &str) -> OutBang {
        self.add_out_assist(self.outlet_count, assist);
        self.add_bang_outlet()
    }

    /// Add an outlet that outputs floats.
    fn add_float_outlet(&mut self) -> OutFloat {
        self.outlet_count += 1;
        Outlet::append_float(self.max_obj)
    }

    fn add_float_outlet_with_assist(&mut self, assist: &str) -> OutFloat {
        self.add_out_assist(self.outlet_count, assist);
        self.add_float_outlet()
    }

    /// Add an outlet that outputs ints.
    fn add_int_outlet(&mut self) -> OutInt {
        self.outlet_count += 1;
        Outlet::append_int(self.max_obj)
    }

    fn add_int_outlet_with_assist(&mut self, assist: &str) -> OutInt {
        self.add_out_assist(self.outlet_count, assist);
        self.add_int_outlet()
    }

    /// Add an outlet that outputs lists.
    fn add_list_outlet(&mut self) -> OutList {
        self.outlet_count += 1;
        Outlet::append_list(self.max_obj)
    }

    fn add_list_outlet_with_assist(&mut self, assist: &str) -> OutList {
        self.add_out_assist(self.outlet_count, assist);
        self.add_list_outlet()
    }

    /// Add an outlet that outputs anything Max supports.
    fn add_anything_outlet(&mut self) -> OutAnything {
        self.outlet_count += 1;
        Outlet::append_anything(self.max_obj)
    }

    fn add_anything_outlet_with_assist(&mut self, assist: &str) -> OutAnything {
        self.add_out_assist(self.outlet_count, assist);
        self.add_anything_outlet()
    }

    fn creation_args(&self) -> &[Atom] {
        self.args
    }
    fn creation_symbol(&self) -> SymbolRef {
        self.sym.clone()
    }
    fn try_register(
        &self,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Registration, RegistrationError> {
        unsafe { Registration::try_register(self.max_obj, namespace, name) }
    }
    fn attach(
        &mut self,
        namespace: SymbolRef,
        name: SymbolRef,
    ) -> Result<Attachment, AttachmentError> {
        unsafe { Attachment::try_attach(self.max_obj, namespace, name) }
    }
    fn subscribe(
        &mut self,
        namespace: SymbolRef,
        name: SymbolRef,
        class_name: Option<SymbolRef>,
    ) -> Subscription {
        unsafe { Subscription::new(self.max_obj, namespace, name, class_name) }
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

    /// Add an inlet with an assist string, left to right, returns index.
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_inlet_with_assist(&mut self, inlet_type: MaxInlet<T>, assist: &str) -> usize {
        let idx = self.add_inlet(inlet_type);
        self.add_in_assist(idx, assist);
        idx
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
        self.outlet_count += count;
    }

    fn add_signal_outlets_with_assist(&mut self, assist: &[&str]) {
        let start = self.outlet_count;
        self.add_signal_outlets(assist.len());
        for (i, s) in assist.iter().enumerate() {
            self.add_out_assist(i + start, *s);
        }
    }

    /// Add signal inlets.
    /// # Panics
    /// * Will panic if called more than once.
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

    fn add_signal_inlets_with_assist(&mut self, assist: &[&str]) {
        let start = self.inlets.len();
        self.add_signal_inlets(assist.len());
        for (i, s) in assist.iter().enumerate() {
            self.add_in_assist(i + start, *s);
        }
    }

    /// Add an inlet, left to right, returns index.
    fn add_inlet(&mut self, inlet_type: MSPInlet<T>) -> usize {
        self.inlets.push(inlet_type);
        self.inlets.len() //there is a default inlet that we don't specify
    }

    /// Add an inlet with an assist string, left to right, returns index.
    /// # Panics
    /// * Will panic if `assist` cannot be converted into a CString
    fn add_inlet_with_assist(&mut self, inlet_type: MSPInlet<T>, assist: &str) -> usize {
        let idx = self.add_inlet(inlet_type);
        self.add_in_assist(idx, assist);
        idx
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
    pub buffer_refs: Vec<ManagedBufferRefInternal>,
    pub assist_ins: HashMap<usize, CString>,
    pub assist_outs: HashMap<usize, CString>,
}

pub struct MSPWrappedBuilderFinalize<T> {
    pub signal_inlets: usize,
    pub signal_outlets: usize,
    pub callbacks_float: FloatCBHash<T>,
    pub callbacks_int: IntCBHash<T>,
    pub proxy_inlets: Vec<Proxy>,
    pub buffer_refs: Vec<ManagedBufferRefInternal>,
    pub assist_ins: HashMap<usize, CString>,
    pub assist_outs: HashMap<usize, CString>,
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
            buffer_refs: self.buffer_refs,
            assist_ins: self.assist_ins,
            assist_outs: self.assist_outs,
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
            buffer_refs: self.buffer_refs,
            assist_ins: self.assist_ins,
            assist_outs: self.assist_outs,
        }
    }
}
