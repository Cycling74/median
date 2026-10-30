use median::prelude::*;

pub struct BadOptions;

#[median::methods]
impl BadOptions {
    #[sel("old", gimme)]
    fn old(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}

    #[gimme("defaults", defaults = 1)]
    fn defaults(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}

    #[gimme]
    fn missing(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}

    #[gimme(42)]
    fn number(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}

    #[gimme("bad\0name")]
    fn nul(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}
}

pub struct Duplicate;

#[median::methods]
impl Duplicate {
    #[sel("same")]
    fn typed(&self) {}

    #[gimme("same")]
    fn atoms(&self, _sel: &SymbolRef, _atoms: &[Atom]) {}
}

fn main() {}
