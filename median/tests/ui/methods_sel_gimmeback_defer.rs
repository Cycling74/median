use median::prelude::*;

struct Delayed;

#[median::methods]
impl Delayed {
    // the reply goes back to the caller: nothing to defer
    #[sel_gimmeback("later")]
    #[defer]
    fn later(&self, _atoms: &[Atom]) -> t_atom_long {
        0
    }

    #[sel_gimmeback("low")]
    #[defer_low]
    fn low(&self, _atoms: &[Atom]) -> t_atom_long {
        0
    }
}

fn main() {}
