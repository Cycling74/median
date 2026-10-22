use median::prelude::*;

//two classes in one external: a Max object and an MSP object. Max finds the second through the
//`init` file next to this crate, which maps its name to this external.

#[derive(MaxObject)]
#[max(name = "median.multi", no_methods)]
pub struct Base {}

impl MaxObjWrapped for Base {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        post!("created median.multi");
        Ok(Self {})
    }
}

#[derive(MaxObject)]
#[max(name = "median.multi.sig~", kind = msp, no_methods)]
pub struct Sig {}

impl MSPObjWrapped for Sig {
    fn new(builder: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        post!("created median.multi.sig~");
        builder.inlet(Inlet::signal());
        builder.inlet(Inlet::signal());
        builder.outlet(Outlet::signal());
        builder.outlet(Outlet::signal());
        Ok(Self {})
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
