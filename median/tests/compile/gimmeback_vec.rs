use median::prelude::*;
use median_mock as _;
#[derive(MaxObj)]
pub struct Simp;
impl MaxObjWrapped for Simp {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Simp)
    }
}
#[median::methods]
impl Simp {
    #[gimmeback("vec")]
    // ACCEPT
    fn method(&self, _atoms: &[Atom]) -> t_atom_long {
        0
    }
    // REJECT: fn method(&self, _atoms: Vec<Atom>) -> t_atom_long { 0 }
}
median::ext_main! { Simp }
fn main() {}
