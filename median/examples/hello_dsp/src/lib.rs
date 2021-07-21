use median::{
    attr::{AttrBuilder, AttrType},
    builder::{MSPWrappedBuilder, ManagedBufferRef},
    class::Class,
    clock::ClockHandle,
    max_sys::t_atom_long,
    num::Int64,
    object::MSPObj,
    post,
    wrapper::{
        attr_get_tramp, attr_set_tramp, tramp, MSPObjWrapped, MSPObjWrapper, WrapperWrapped,
    },
};

median::external! {
    #[name="hello_dsp~"]
    pub struct HelloDSP {
        value: Int64,
        _v: String,
        clock: ClockHandle,
        buffer1: ManagedBufferRef,
        buffer2: ManagedBufferRef
    }

    impl MSPObjWrapped<HelloDSP> for HelloDSP {
        //create some signal i/o
        fn new(builder: &mut dyn MSPWrappedBuilder<Self>) -> Self {
            builder.add_signal_inlets(2);
            builder.add_signal_outlets(2);
            Self {
                value: Int64::new(0),
                _v: String::from("blah"),
                clock: builder.with_clockfn(Self::clocked),
                buffer1: builder.with_buffer(None),
                buffer2: builder.with_buffer(None),
            }
        }

        //perform the dsp
        fn perform(&self, _ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
            let c = if self.buffer1.exists() { 1. } else { 0.} + if self.buffer2.exists() { 1. } else { 0. };
            for o in outs[0].iter_mut() {
                *o = c;
            }
            for o in outs[1].iter_mut() {
                *o = 1f64;
            }
        }

        // Register any methods you need for your class
        fn class_setup(c: &mut Class<MSPObjWrapper<Self>>) {
            //explicitly create a "set" selector method with a single symbol argument
            c.add_method(median::method::Method::SelS(&"set", Self::set_tramp, 0)).unwrap();

            c.add_attribute(
                AttrBuilder::new_accessors(
                    "blah",
                    AttrType::Int64,
                    Self::blah_tramp,
                    Self::set_blah_tramp,
                )
                .build()
                .unwrap(),
            )
                .expect("failed to add attribute");
        }
    }

    impl HelloDSP {
        #[bang]
        pub fn bang(&self) {
            median::object_post!(self.as_max_obj(), "from rust {}", self.value);
            self.clock.delay(10);
        }

        //create a trampoline for the c.add_method in `class_setup` above.
        //Max doesn't accept methods direct to your rust struct, you need to create a "trampoline"
        //that it uses instead. This "trampoline" is a C method that is called on a wrapper struct
        //that in turn calls this "set" method on your wrapped object.
        //The trampoline methods are named with `_tramp` appended to the end.
        #[tramp]
        pub fn set(&self, name: median::symbol::SymbolRef) {
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
                median::object_error!(self.as_max_obj(), "from rust {}", self.value);
            }
        }

        #[attr_get_tramp]
        pub fn blah(&self) -> t_atom_long {
            self.value.get()
        }

        #[attr_set_tramp]
        pub fn set_blah(&self, v: t_atom_long) {
            self.value.set(v);
        }

        pub fn clocked(&self) {
            post("clocked".to_string());
        }
    }

}
