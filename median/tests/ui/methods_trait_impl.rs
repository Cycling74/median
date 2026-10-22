pub struct Simp;

pub trait Other {
    fn bang(&self);
}

#[median::methods]
impl Other for Simp {
    #[bang]
    fn bang(&self) {}
}

fn main() {}
