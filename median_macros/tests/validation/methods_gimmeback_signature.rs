use median::prelude::*;

// what the macro itself refuses, so that nothing reaches the compiler's trait checks
pub struct Simp;

#[median::methods]
impl Simp {
    // three arguments
    #[gimmeback("three")]
    pub fn three(&self, _sel: &SymbolRef, _atoms: &[Atom], _more: &[Atom]) -> t_atom_long {
        0
    }

    // no arguments
    #[gimmeback("none")]
    pub fn none(&self) -> t_atom_long {
        0
    }

    // untyped selector
    #[gimmeback("dblclick")]
    pub fn dblclick(&self, _atoms: &[Atom]) -> t_atom_long {
        0
    }

    // reserved selector
    #[gimmeback("assist")]
    pub fn assist(&self, _atoms: &[Atom]) -> t_atom_long {
        0
    }

    // an option
    #[gimmeback("opt", defaults = 1)]
    pub fn opt(&self, _atoms: &[Atom]) -> t_atom_long {
        0
    }
}

fn main() {}
