use median::prelude::*;
use median_mock as _;
#[derive(MaxObj)]
// ACCEPT
#[max(name = "valid", no_methods)]
// REJECT: #[max(nmae = "valid", no_methods)]
pub struct Simp;
impl MaxObjWrapped for Simp {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Simp)
    }
}
fn main() {}
