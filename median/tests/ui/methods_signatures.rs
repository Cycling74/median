use median::prelude::*;

pub struct Simp;

#[median::methods]
impl Simp {
    #[bang]
    pub fn bang(&self, _extra: t_atom_long) {}

    #[int]
    pub fn int(&self) {}

    #[float]
    pub fn float(&mut self, _v: f64) {}

    #[list]
    pub fn list(&self, _atoms: Vec<Atom>) {}

    #[any]
    pub fn any(&self, _atoms: &[Atom]) {}

    #[sel("gimme", gimme)]
    pub fn gimme(&self, _sel: SymbolRef, _atoms: &[Atom]) {}

    #[sel("eight")]
    pub fn eight(&self, _a: f64, _b: f64, _c: f64, _d: f64, _e: f64, _f: f64, _g: f64, _h: f64) {}

    #[sel("defaults", defaults = 2)]
    pub fn defaults(&self, _a: f64) {}

    #[attr]
    pub fn getter(&self) {}

    #[attr]
    pub fn foo(&self) -> f64 {
        0.
    }

    pub fn set_foo(&self) {}
}

fn main() {}
