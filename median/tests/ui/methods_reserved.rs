use median::prelude::*;

#[derive(MaxObject)]
pub struct Simp;

impl MaxObjWrapped for Simp {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Simp)
    }
}

// selectors the wrapper registers itself
#[median::methods]
impl Simp {
    #[sel("in1")]
    pub fn a(&self, _v: t_atom_long) {}

    #[sel("assist", gimme)]
    pub fn b(&self, _s: &SymbolRef, _a: &[Atom]) {}

    #[sel("notify")]
    pub fn c(&self) {}

    // `class_dspinit`'s: not refused here (the kind isn't known); `Class::add_*` refuses it for
    // an MSP class, a Max class may have it
    #[sel("enable")]
    pub fn e(&self, _v: t_atom_long) {}

    // fine
    #[sel("in10")]
    pub fn d(&self) {}
}

fn main() {}
