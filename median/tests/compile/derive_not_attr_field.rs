use median_mock as _;
// ACCEPT
type Field = median::num::Int64;
// REJECT: type Field = String;
use median::prelude::*;

#[derive(MaxObj)]
#[max(no_methods)]
pub struct Simp {
    #[attr]
    name: Field,
}

impl MaxObjWrapped for Simp {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Simp {
            name: Field::default(),
        })
    }
}

fn main() {}
