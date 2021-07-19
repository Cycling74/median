#[no_mangle]
pub unsafe extern "C" fn atom_setfloat(a: *mut max_sys::t_atom, b: f64) -> max_sys::t_max_err {
    if a.is_null() {
        max_sys::e_max_errorcodes::MAX_ERR_GENERIC as _
    } else {
        (*a).a_type = max_sys::e_max_atomtypes::A_FLOAT as _;
        (*a).a_w.w_float = b;
        max_sys::e_max_errorcodes::MAX_ERR_NONE as _
    }
}

#[no_mangle]
pub unsafe extern "C" fn atom_setlong(
    a: *mut max_sys::t_atom,
    b: max_sys::t_atom_long,
) -> max_sys::t_max_err {
    (*a).a_type = max_sys::e_max_atomtypes::A_LONG as _;
    (*a).a_w.w_long = b;
    max_sys::e_max_errorcodes::MAX_ERR_NONE as _
}
