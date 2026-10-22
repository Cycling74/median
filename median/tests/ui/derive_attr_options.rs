use median::prelude::*;

#[derive(MaxObject)]
pub struct SaveTwice {
    #[attr(save, label = "Value", save)]
    value: Int64,
}

#[derive(MaxObject)]
pub struct LabelNotAString {
    #[attr(label = 5)]
    value: Int64,
}

#[derive(MaxObject)]
pub struct BadRange {
    #[attr(range(1))]
    value: Int64,
}

#[derive(MaxObject)]
pub struct UnknownOption {
    #[attr(clamp(0, 1))]
    value: Int64,
}

#[derive(MaxObject)]
pub struct BadClip {
    #[attr(clip(0))]
    value: Int64,
}

#[derive(MaxObject)]
pub struct Twice {
    #[attr]
    a: Int64,
    #[attr(name = "a")]
    b: Int64,
}

fn main() {}
