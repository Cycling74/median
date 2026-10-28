use median::prelude::*;

// signal inlets and outlets are for MSP objects only
#[derive(MaxObj)]
#[max(no_methods)]
pub struct Simp;

impl MaxObjWrapped for Simp {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::signal());
        builder.outlet(Outlet::signal());
        Ok(Simp)
    }
}

fn main() {}
