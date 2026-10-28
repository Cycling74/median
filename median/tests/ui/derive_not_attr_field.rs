use median::prelude::*;

#[derive(MaxObj)]
#[max(no_methods)]
pub struct Simp {
    #[attr]
    name: String,
}

impl MaxObjWrapped for Simp {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Simp { name: String::new() })
    }
}

fn main() {}
