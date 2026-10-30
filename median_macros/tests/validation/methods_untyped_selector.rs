use median::prelude::*;

#[derive(MaxObj)]
pub struct Simp;

impl MaxObjWrapped for Simp {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Simp)
    }
}

// untyped messages: Max calls them with its own arguments, so a typed selector would misread them
#[median::methods]
impl Simp {
    #[sel("appendtodictionary")]
    pub fn a(&self, _s: SymbolRef) {}

    #[gimme("loadbang")]
    pub fn b(&self, _s: &SymbolRef, _a: &[Atom]) {}

    #[sel("dblclick")]
    pub fn c(&self) {}

    #[gimme("getvalueof")]
    pub fn e(&self, _s: &SymbolRef, _a: &[Atom]) {}

    // fine
    #[sel("dblclicked")]
    pub fn d(&self) {}
}

fn main() {}
