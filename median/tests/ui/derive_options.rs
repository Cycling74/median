use median::prelude::*;

#[derive(MaxObject)]
#[max(nmae = "x")]
pub struct UnknownOption;

#[derive(MaxObject)]
#[max(kind = jitter)]
pub struct UnknownKind;

#[derive(MaxObject)]
#[max(name = "")]
pub struct EmptyName;

#[derive(MaxObject)]
pub struct Generic<T> {
    t: T,
}

fn main() {}
