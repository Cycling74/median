use median::prelude::*;

// `kind = msp`, but implements the Max trait
#[derive(MaxObject)]
#[max(kind = msp, no_methods)]
pub struct Simp;

impl MaxObjWrapped for Simp {
    fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
        Simp
    }
}

fn main() {}
