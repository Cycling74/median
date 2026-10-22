use median::prelude::*;

#[derive(MaxObject)]
pub struct Simp;

impl MaxObjWrapped for Simp {
    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Simp)
    }
}

#[median::methods]
impl Simp {
    // not `()` or `Result<(), E: Display>`
    #[bang]
    pub fn bang(&self) -> String {
        String::new()
    }

    // the wrong argument type
    #[int]
    pub fn int(&self, _v: f64) {}
}

fn main() {}
