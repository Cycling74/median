use median_mock as _;
// `sym!` checks its literal at compile time: an interior NUL byte is an error.

fn main() {
    let _ok = median::sym!("fine");
    // ACCEPT
    let _bad = median::sym!("ab");
    // REJECT: let _bad = median::sym!("a\0b");
}
