use median::prelude::*;

pub struct Simp;

#[median::methods]
impl Simp {
    // Max passes the float and the long in different registers
    #[sel("range")]
    pub fn range(&self, _lo: f64, _hi: t_atom_long) {}

    // all floats, or none, is fine
    #[sel("floats")]
    pub fn floats(&self, _a: f64, _b: f64) {}

    #[sel("others")]
    pub fn others(&self, _a: t_atom_long, _b: SymbolRef) {}
}

fn main() {}
