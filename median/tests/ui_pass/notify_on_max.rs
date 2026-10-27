// built (not only checked), because trybuild only builds when a suite has a passing case and
// the post-monomorphisation checks of the `jitter_notify` case need that: a `#[notify]` method
// on an ordinary Max type must keep compiling
use median::{notify::Notification, prelude::*};
use median_mock as _;

#[derive(MaxObject)]
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
