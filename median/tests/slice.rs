mod common;

use median::{atom::Atom, slice::Slice};
use std::convert::From;

#[test]
fn can_create_slice() {
    let s: Slice<Atom> = Slice::from([0i64, 1i64].iter());
    assert_eq!(2, s.len());

    let (p, l) = s.into_raw();
    assert!(!p.is_null());
    assert_eq!(2, l);

    let s = Slice::from_raw_parts_mut(p, l);
    assert_eq!(2, s.len());
}
