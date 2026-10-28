use median::prelude::*;

#[derive(MaxObj)]
#[max(description = 5)]
pub struct NotAString;

#[derive(MaxObj)]
#[max(author = "a", author = "b")]
pub struct Twice;

#[derive(MaxObj)]
#[max(tags = "a, , b")]
pub struct EmptyTag;

#[derive(MaxObj)]
#[max(tags = "")]
pub struct NoTags;

fn main() {}
