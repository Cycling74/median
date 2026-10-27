//! `hello_dsp~`: an MSP example with two signal inlets and outlets, an attribute and two
//! `buffer~` references.
//!
//! In Max:
//! * `[hello_dsp~ foo bar]` follows the `buffer~`s named `foo` (buffer1) and `bar` (buffer2); both
//!   arguments are optional;
//! * `set <name>` points buffer1 at another `buffer~`, `set2 <name>` buffer2;
//! * double-clicking the box opens buffer1's window;
//! * the left outlet is the number of the two buffers that exist (0, 1 or 2), the right outlet
//!   plays the first channel of buffer1 (1 when it has no `buffer~`).

use median::{args::FromAtom, prelude::*};

#[derive(MaxObject)]
#[max(name = "hello_dsp~", kind = msp)]
pub struct HelloDSP {
    obj: ObjRef,
    //an attribute "blah" backed by this field, shown as "Blah" on the inspector's "Hello" tab
    //and saved with the patcher
    #[attr(name = "blah", label = "Blah", category = "Hello", save)]
    value: Int64,
    _v: String,
    clock: Clock,
    buffer1: ManagedBufferRef,
    buffer2: ManagedBufferRef,
}

impl MSPObjWrapped for HelloDSP {
    //create some signal i/o
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        //the buffer~ names, from the leading symbol arguments of the positional ones (`@name value`
        //arguments are attributes, which are set after `new`)
        let mut names = builder
            .args()
            .positional()
            .iter()
            .map_while(|a| SymbolRef::from_atom(a).ok().filter(|s| !s.is_empty()));
        let (name1, name2) = (names.next(), names.next());

        //signal inlets come first; without `.assist(..)` Max shows "signal"
        builder.inlet(Inlet::signal().assist("in left; set <buffer~>, set2 <buffer~>"));
        builder.inlet(Inlet::signal().assist("in right"));
        builder.outlet(Outlet::signal().assist("out left"));
        builder.outlet(Outlet::signal().assist("out right"));
        Ok(Self {
            obj: builder.obj(),
            value: Int64::new(0),
            _v: String::from("blah"),
            clock: builder.clock(Self::clocked),
            buffer1: builder.with_buffer(name1),
            buffer2: builder.with_buffer(name2),
        })
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

    //`set <name>` points buffer1 at another buffer~
    #[sel("set")]
    pub fn set(&self, name: SymbolRef) {
        self.buffer1.set(name);
    }

    //`set2 <name>` points buffer2 at another buffer~
    #[sel("set2")]
    pub fn set2(&self, name: SymbolRef) {
        self.buffer2.set(name);
    }

    //double-clicking the box opens buffer1's window
    #[dblclick]
    pub fn dblclick(&self) {
        self.buffer1.view();
    }

    pub fn clocked(&self) {
        post("clocked".to_string());
    }
}

median::ext_main! { HelloDSP }
