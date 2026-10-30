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
    #[bang]
    // ACCEPT
    fn method(&self) {}
    // REJECT: fn method(&self) -> String { String::new() }
}
median::ext_main! { Simp }
fn main() {}
