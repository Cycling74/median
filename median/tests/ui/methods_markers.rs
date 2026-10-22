use median::prelude::*;

pub struct Simp;

#[median::methods]
impl Simp {
    #[bang]
    pub fn bang(&self) {}

    #[bang]
    pub fn bang_again(&self) {}

    #[bang]
    #[int]
    pub fn two(&self) {}

    #[defer]
    pub fn later(&self) {}

    #[sel("twice")]
    #[defer]
    #[defer_low]
    pub fn twice(&self) {}

    #[attr]
    #[defer_low]
    pub fn deferred_attr(&self) -> f64 {
        0.
    }

    #[dblclick]
    #[defer]
    pub fn open(&self) {}

    #[loadbang]
    pub fn loaded(&self, _v: t_atom_long) {}

    #[savestate]
    pub fn save(&self) {}

    #[bang(now)]
    pub fn args(&self) {}

    #[sel("set", sometimes)]
    pub fn set(&self) {}

    #[attr(style = "onoff")]
    pub fn toggle(&self) -> t_atom_long {
        0
    }
}

fn main() {}
