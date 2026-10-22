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
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        //signal inlets come first; without `.assist(..)` Max shows "signal"
        builder.inlet(Inlet::signal().assist("in left"));
        builder.inlet(Inlet::signal().assist("in right"));
        builder.outlet(Outlet::signal().assist("out left"));
        builder.outlet(Outlet::signal().assist("out right"));
        Ok(Self {
            obj: builder.obj(),
            value: Int64::new(0),
            _v: String::from("blah"),
            clock: builder.with_clockfn(Self::clocked),
            buffer1: builder.with_buffer(None),
            buffer2: builder.with_buffer(None),
        })
    }

    //registered after the `#[median::methods]` impl
    fn class_setup(c: &mut Class<MSPObjWrapper<Self>>) {
        //`set <name>` points buffer1 at another buffer~, double-clicking the box opens its window
        c.add_buffer_messages(|s: &Self| &*s.buffer1)
            .expect("set and dblclick");
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
        //the right outlet: the first channel of buffer1 from its start (repeated if the buffer
        //is shorter than the vector), or 1 without a buffer. `try_lock` neither allocates nor
        //locks; the claim is released when `buf` goes out of scope.
        match self.buffer1.try_lock() {
            Ok(buf) if buf.frames() > 0 && buf.channels() > 0 => {
                //interleaved data: a channel is a strided view
                for (o, s) in outs[1].iter_mut().zip(buf.channel_iter(0).cycle()) {
                    *o = s.get() as f64;
                }
            }
            Ok(_) => outs[1].fill(0f64),
            Err(_) => outs[1].fill(1f64),
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
