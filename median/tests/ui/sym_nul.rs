//! `sym!` checks its literal at compile time: an interior NUL byte is an error.

fn main() {
    let _ok = median::sym!("fine");
    let _bad = median::sym!("a\0b");
}
