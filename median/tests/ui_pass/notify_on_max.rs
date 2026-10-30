// An ordinary Max object may register notify; the Jitter rejection must not affect it.
use median::{notify::Notification, prelude::*};
use median_mock as _;

#[derive(MaxObj)]
#[max(name = "pass.notify")]
struct Plain;

impl MaxObjWrapped for Plain {
    fn new(_b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Plain)
    }
}

#[median::methods]
impl Plain {
    #[notify]
    fn notified(&self, _n: &Notification) {}
}

median::ext_main! { Plain }

fn main() {}
