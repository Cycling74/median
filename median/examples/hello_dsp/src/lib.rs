use median::{builder::ManagedBufferRef, prelude::*};

#[derive(MaxObject)]
#[max(name = "hello_dsp~", kind = msp)]
pub struct HelloDSP {
    obj: ObjRef,
    //an attribute "blah" backed by this field
    #[attr(name = "blah")]
    value: Int64,
    _v: String,
    clock: ClockHandle,
    buffer1: ManagedBufferRef,
    buffer2: ManagedBufferRef,
}

impl MSPObjWrapped for HelloDSP {
    //create some signal i/o
    fn new(builder: &mut dyn MSPWrappedBuilder<Self>) -> Self {
        builder.add_signal_inlets_with_assist(&["in left", "in right"]);
        //without assist it would be builder.add_signal_inlets(2);
        builder.add_signal_outlets_with_assist(&["out left", "out right"]);
        Self {
            obj: builder.obj(),
            value: Int64::new(0),
            _v: String::from("blah"),
            clock: builder.with_clockfn(Self::clocked),
            buffer1: builder.with_buffer(None),
            buffer2: builder.with_buffer(None),
        }
    }

    fn dsp_setup(&self, sample_rate: f64) {
        median::object_post!(&self.obj, "sample rate: {}", sample_rate);
    }

    //perform the dsp
    fn perform(&self, _ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        let c = if self.buffer1.exists() { 1. } else { 0. }
            + if self.buffer2.exists() { 1. } else { 0. };
        for o in outs[0].iter_mut() {
            *o = c;
        }
        for o in outs[1].iter_mut() {
            *o = 1f64;
        }
    }
}

#[median::methods]
impl HelloDSP {
    #[bang]
    pub fn bang(&self) {
        median::object_post!(&self.obj, "from rust {}", self.value);
        self.clock.delay(10);
    }

    //a "set" selector method with a single symbol argument
    #[sel("set")]
    pub fn set(&self, name: SymbolRef) {
        self.buffer1.set(name);
    }

    #[int]
    pub fn int(&self, v: t_atom_long) {
        self.value.set(v);
        //XXX won't compile, needs mutex
        //self._v = format!("from rust {}", self.value);
        post!("from rust {}", self.value);
        //just an example to show an error
        if v < 0 {
            median::object_error!(&self.obj, "from rust {}", self.value);
        }
    }

    pub fn clocked(&self) {
        post("clocked".to_string());
    }
}

median::ext_main! { HelloDSP }
