use median::prelude::*;
use median_mock as _;
#[derive(MaxObj)]
#[max(no_methods)]
pub struct Simp;
impl MaxObjWrapped for Simp {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        // ACCEPT
        builder.outlet(Outlet::list());
        // REJECT: builder.outlet(Outlet::signal());
        Ok(Simp)
    }
}
fn main() {}
