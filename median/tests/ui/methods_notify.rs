use median::notify::Notification;

struct Bad;

#[median::methods]
impl Bad {
    #[notify]
    fn no_argument(&self) {}

    #[notify]
    fn by_value(&self, _n: Notification) {}
}

struct Delayed;

#[median::methods]
impl Delayed {
    #[defer]
    #[notify]
    fn later(&self, _n: &Notification) {}
}

struct Twice;

#[median::methods]
impl Twice {
    #[notify]
    fn first(&self, _n: &Notification) {}

    #[notify]
    fn second(&self, _n: &Notification) {}
}

fn main() {}
