use median::dict::Dict;

struct Bad;

#[median::methods]
impl Bad {
    #[dictionary]
    fn borrowed(&self, _dict: &Dict) {}

    #[savestate]
    fn owned(&self, _dict: Dict) {}

    #[defer]
    #[dictionary]
    fn delayed(&self, _dict: Dict) {}
}

fn main() {}
