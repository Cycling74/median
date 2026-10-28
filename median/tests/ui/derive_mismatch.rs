use median::prelude::*;

// `#[derive(MSPObj)]`, but implements the Max trait
#[derive(MSPObj)]
#[max(no_methods)]
pub struct Simp;

impl MaxObjWrapped for Simp {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Simp)
    }
}

fn main() {}
