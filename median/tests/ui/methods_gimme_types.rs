use median::prelude::*;

pub struct Simp;

#[median::methods]
impl Simp {
    // not atoms: the `GimmeFn` bound of `add_gimme` refuses it at the method
    #[gimme("vec")]
    pub fn vec(&self, _atoms: Vec<Atom>) {}

    // the selector by value
    #[gimme("by_value")]
    pub fn by_value(&self, _sel: SymbolRef, _atoms: &[Atom]) {}

    // a result other than `()` or `Result<(), E>`
    #[gimme("returns")]
    pub fn returns(&self, _atoms: &[Atom]) -> i32 {
        0
    }
}

fn main() {}
