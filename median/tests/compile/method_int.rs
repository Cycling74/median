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
    #[int]
    // ACCEPT
    fn method(&self, _v: t_atom_long) {}
    // REJECT: fn method(&self, _v: f64) {}
}
median::ext_main! { Simp }
fn main() {}
