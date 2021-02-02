mod common;

use median::symbol::SymbolRef;
use std::convert::TryFrom;

#[test]
fn can_create_symbol() {
    common::with_setup(|| {
        let t = SymbolRef::try_from("toast");
        assert!(t.is_ok());
        let t = SymbolRef::try_from("toast2");
        assert!(t.is_ok());
    });
}
