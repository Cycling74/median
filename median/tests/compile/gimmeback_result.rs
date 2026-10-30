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
    #[gimmeback("res")]
    // ACCEPT
    fn method(&self, _atoms: &[Atom]) -> Result<t_atom_long, String> {
        Ok(0)
    }
    // REJECT: fn method(&self, _atoms: &[Atom]) -> Result<i32, String> { Ok(0) }
}
median::ext_main! { Simp }
fn main() {}
