use median::{
    attr::{AttrBuilder, AttrType},
    builder::{MSPWrappedBuilder, ManagedBufferRef},
    class::Class,
    clock::ClockHandle,
    num::Int64,
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

        fn perform(&self, _ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
            let c = if self.buffer1.exists() { 1. } else { 0.} + if self.buffer2.exists() { 1. } else { 0. };
            for o in outs[0].iter_mut() {
                *o = c;
            }
            for o in outs[1].iter_mut() {
                *o = 1f64;
            }
        }

        /// Register any methods you need for your class
        fn class_setup(c: &mut Class<MSPObjWrapper<Self>>) {
            c.add_method(median::method::Method::Int(Self::int_tramp)).unwrap();
            c.add_method(median::method::Method::Bang(Self::bang_tramp)).unwrap();
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

    type Wrapper = MSPObjWrapper<HelloDSP>;

    impl HelloDSP {
        #[tramp(Wrapper)]
        pub fn bang(&self) {
            post!("from rust {}", self.value);
            self.clock.delay(10);
        }

        #[tramp(Wrapper)]
        pub fn set(&self, name: *mut max_sys::t_symbol) {
            let name = name.into();
            self.buffer1.set(name);
        }

        #[tramp(Wrapper)]
        pub fn int(&self, v: i64) {
            self.value.set(v);
            //XXX won't compile, needs mutex
            //self._v = format!("from rust {}", self.value);
            post!("from rust {}", self.value);
        }

        #[attr_get_tramp(Wrapper)]
        pub fn blah(&self) -> i64 {
            self.value.get()
        }

        #[attr_set_tramp(Wrapper)]
        pub fn set_blah(&self, v: i64) {
            self.value.set(v);
        }

        pub fn clocked(&self) {
            post("clocked".to_string());
        }
    }

}
