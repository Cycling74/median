use median::{
    attr::{AttrBuilder, AttrType},
    builder::MSPWrappedBuilder,
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
        pub value: Int64,
        _v: String,
        clock: ClockHandle,
    }

    impl MSPObjWrapped<HelloDSP> for HelloDSP {
        fn new(builder: &mut dyn MSPWrappedBuilder<Self>) -> Self {
            builder.add_signal_inlets(2);
            builder.add_signal_outlets(2);
            Self {
                value: Int64::new(0),
                _v: String::from("blah"),
                clock: builder.with_clockfn(Self::clocked),
            }
        }

        fn perform(&self, _ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
            for o in outs[0].iter_mut() {
                *o = 2f64;
            }
            for o in outs[1].iter_mut() {
                *o = 1f64;
            }
        }

        /// Register any methods you need for your class
        fn class_setup(c: &mut Class<MSPObjWrapper<Self>>) {
            c.add_method(median::method::Method::Int(Self::int_tramp));
            c.add_method(median::method::Method::Bang(Self::bang_tramp));

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
