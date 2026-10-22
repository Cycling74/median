use median::prelude::*;

// an outlet can't be duplicated, so a copy can't outlive the object
#[derive(MaxObject)]
#[max(no_methods)]
pub struct Simp {
    out: Outlet<List>,
}

impl MaxObjWrapped for Simp {
    fn new(builder: &mut MaxBuilder<Self>) -> Self {
        let out = builder.outlet(Outlet::list());
        let _copy = out.clone();
        Self { out }
    }
}

fn main() {}
