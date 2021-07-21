use median::{
    builder::{MSPWrappedBuilder, MaxWrappedBuilder},
    post,
    wrapper::{MSPObjWrapped, MaxObjWrapped},
};

//create an external but don't implement ext_main (we do it explicitly below)
median::external_no_main! {
    #[name="median.multi"]
    pub struct Base { }

    impl MaxObjWrapped<Base> for Base {
        fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
            post!("created median.multi");
            Self { }
        }
    }
}

median::external_no_main! {
    #[name="median.multi.sig~"]
    pub struct Sig { }

    impl MSPObjWrapped<Sig> for Sig {
        fn new(builder: &mut dyn MSPWrappedBuilder<Self>) -> Self {
            post!("created median.multi.sig~");
            builder.add_signal_inlets(2);
            builder.add_signal_outlets(2);
            Self { }
        }
        fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
            for (outv, inv) in outs.iter_mut().zip(ins.iter()) {
                for (o, i) in outv.iter_mut().zip(inv.iter()) {
                    *o = *i;
                }
            }
        }
    }
}

//create an ext_main, register our objects
median::ext_main! {
    Base::register();
    Sig::register();
}
