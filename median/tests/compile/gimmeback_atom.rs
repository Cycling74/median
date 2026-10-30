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
    #[gimmeback("atom")]
    // ACCEPT
    fn method(&self, _atoms: &[Atom]) -> t_atom_long {
        1
    }
    // REJECT: fn method(&self, _atoms: &[Atom]) -> Atom { Atom::from(1 as t_atom_long) }
}
median::ext_main! { Simp }
fn main() {}
