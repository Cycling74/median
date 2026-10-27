use median::prelude::*;

pub struct Simp;

#[median::methods]
impl Simp {
    // a bare atom is not a reply: it could carry any pointer
    #[sel_gimmeback("atom")]
    pub fn atom(&self, _atoms: &[Atom]) -> Atom {
        Atom::from(1 as t_atom_long)
    }

    // `i32` does not convert to a `Reply` (`t_atom_long` does)
    #[sel_gimmeback("int")]
    pub fn int(&self, _atoms: &[Atom]) -> i32 {
        0
    }

    // not atoms
    #[sel_gimmeback("vec")]
    pub fn vec(&self, _atoms: Vec<Atom>) -> t_atom_long {
        0
    }

    // a result with a non-convertible value
    #[sel_gimmeback("res")]
    pub fn res(&self, _atoms: &[Atom]) -> Result<i32, String> {
        Ok(0)
    }
}

fn main() {}
