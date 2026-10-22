use median::prelude::*;

// signal inlets and outlets are for MSP objects only
#[derive(MaxObject)]
#[max(no_methods)]
pub struct Simp;

impl MaxObjWrapped for Simp {
    fn new(builder: &mut MaxBuilder<Self>) -> Self {
        builder.inlet(Inlet::signal());
        builder.outlet(Outlet::signal());
        Simp
    }
}

fn main() {}
