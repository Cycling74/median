use median::prelude::*;

//two classes in one external: a Max object and an MSP object. Max finds the second through the
//`init` file next to this crate, which maps its name to this external.

#[derive(MaxObject)]
#[max(name = "median.multi", no_methods)]
pub struct Base {}

impl MaxObjWrapped for Base {
    fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
        post!("created median.multi");
        Self {}
    }
}

#[derive(MaxObject)]
#[max(name = "median.multi.sig~", kind = msp, no_methods)]
pub struct Sig {}

impl MSPObjWrapped for Sig {
    fn new(builder: &mut dyn MSPWrappedBuilder<Self>) -> Self {
        post!("created median.multi.sig~");
        builder.add_signal_inlets(2);
        builder.add_signal_outlets(2);
        Self {}
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        for (outv, inv) in outs.iter_mut().zip(ins.iter()) {
            for (o, i) in outv.iter_mut().zip(inv.iter()) {
                *o = *i;
            }
        }
    }
}

//one `ext_main` that registers both, each on its own
median::ext_main! { Base, Sig }
