use median::prelude::*;
use median_mock as _;

// `#[derive(MSPObj)]`, but implements the Max trait
// ACCEPT
#[derive(MaxObj)]
// REJECT: #[derive(MSPObj)]
#[max(no_methods)]
pub struct Simp;

impl MaxObjWrapped for Simp {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Simp)
    }
}

fn main() {}
