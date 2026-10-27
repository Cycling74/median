use median::prelude::*;

#[derive(MaxObject)]
#[max(description = 5)]
pub struct NotAString;

#[derive(MaxObject)]
#[max(author = "a", author = "b")]
pub struct Twice;

#[derive(MaxObject)]
#[max(tags = "a, , b")]
pub struct EmptyTag;

#[derive(MaxObject)]
#[max(tags = "")]
pub struct NoTags;

fn main() {}
