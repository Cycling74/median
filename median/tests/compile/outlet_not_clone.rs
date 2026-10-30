use median::prelude::*;
use median_mock as _;

// an outlet can't be duplicated, so a copy can't outlive the object
#[derive(MaxObj)]
#[max(no_methods)]
pub struct Simp {
    out: Outlet<List>,
}

impl MaxObjWrapped for Simp {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let out = builder.outlet(Outlet::list());
        // ACCEPT
        let _copy = &out;
        // REJECT: let _copy = out.clone();
        Ok(Self { out })
    }
}

fn main() {}
