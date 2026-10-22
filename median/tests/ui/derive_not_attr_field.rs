use median::prelude::*;

#[derive(MaxObject)]
#[max(no_methods)]
pub struct Simp {
    #[attr]
    name: String,
}

impl MaxObjWrapped for Simp {
    fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
        Simp { name: String::new() }
    }
}

fn main() {}
