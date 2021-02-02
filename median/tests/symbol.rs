mod common;

use median::symbol::SymbolRef;
use std::convert::TryFrom;

#[test]
fn can_create_symbol() {
    common::setup();
    let _t = SymbolRef::try_from("toast");
}

#[test]
fn can_create_symbol_again() {
    common::setup();
    let t = SymbolRef::try_from("toast2");
    assert!(t.is_ok());
    let t = SymbolRef::try_from("toast2");
    assert!(t.is_ok());
}
