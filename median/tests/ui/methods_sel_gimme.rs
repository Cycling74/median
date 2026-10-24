use median::prelude::*;

pub struct BadOptions;

#[median::methods]
impl BadOptions {
    #[sel("old", gimme)]
    fn old(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}

    #[sel_gimme("defaults", defaults = 1)]
    fn defaults(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}

    #[sel_gimme]
    fn missing(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}

    #[sel_gimme(42)]
    fn number(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}

    #[sel_gimme("bad\0name")]
    fn nul(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}
}

pub struct Duplicate;

#[median::methods]
impl Duplicate {
    #[sel("same")]
    fn typed(&self) {}

    #[sel_gimme("same")]
    fn atoms(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}
}

fn main() {}
