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
    #[gimme("vec")]
    // ACCEPT
    fn method(&self, _atoms: &[Atom]) {}
    // REJECT: fn method(&self, _atoms: Vec<Atom>) {}
}
median::ext_main! { Simp }
fn main() {}
