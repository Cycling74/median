use median::prelude::*;

#[derive(MaxObj)]
#[max(nmae = "x")]
pub struct UnknownOption;

#[derive(MaxObj)]
#[max(kind = msp)]
pub struct UnknownKind;

#[derive(MaxObj)]
#[max(name = "")]
pub struct EmptyName;

#[derive(MaxObj)]
pub struct Generic<T> {
    t: T,
}

fn main() {}
