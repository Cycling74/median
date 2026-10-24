//! Exercise the C variadic entry point, including mixed register classes and Windows C long.
use core::ffi::{c_long, c_void};
use max_sys::{self as sys, e_max_atomtypes as ty, t_atom, t_atom_long, t_object, t_symbol};
use std::sync::Mutex;

type Call = (t_atom_long, f64, usize, usize, t_atom_long);
static CALLS: Mutex<Vec<Call>> = Mutex::new(Vec::new());
extern "C" fn zero(x: *mut t_object) -> *mut c_void {
    x.cast()
}
extern "C" fn mixed(
    _: *mut t_object,
    a: t_atom_long,
    b: f64,
    c: *mut t_symbol,
    d: *mut c_void,
    e: t_atom_long,
) -> *mut c_void {
    CALLS
        .lock()
        .unwrap()
        .push((a, b, c as usize, d as usize, e));
    d
}
extern "C" fn gimme(_: *mut t_object, s: *mut t_symbol, n: c_long, a: *mut t_atom) -> *mut c_void {
    CALLS
        .lock()
        .unwrap()
        .push((n as _, 0., s as usize, a as usize, 0));
    a.cast()
}

#[test]
fn reads_only_registered_arguments_and_rejects_unknown_signatures() {
    let k = median_mock::lock();
    CALLS.lock().unwrap().clear();
    unsafe {
        let c = sys::class_new(
            c"variadic.test".as_ptr(),
            None,
            None,
            std::mem::size_of::<t_object>() as c_long,
            None,
            0,
        );
        let zero_method = Some(std::mem::transmute::<
            extern "C" fn(*mut t_object) -> *mut c_void,
            unsafe extern "C" fn(*mut c_void) -> *mut c_void,
        >(zero));
        sys::class_addmethod(c, zero_method, c"zero".as_ptr(), 0);
        sys::class_addmethod(c, zero_method, c"untyped".as_ptr(), ty::A_CANT, 0);
        sys::class_addmethod(
            c,
            Some(std::mem::transmute::<
                extern "C" fn(
                    *mut t_object,
                    t_atom_long,
                    f64,
                    *mut t_symbol,
                    *mut c_void,
                    t_atom_long,
                ) -> *mut c_void,
                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            >(mixed)),
            c"mixed".as_ptr(),
            ty::A_LONG,
            ty::A_DEFFLOAT,
            ty::A_SYM,
            ty::A_OBJ,
            ty::A_DEFLONG,
            0,
        );
        sys::class_addmethod(
            c,
            Some(std::mem::transmute::<
                extern "C" fn(*mut t_object, *mut t_symbol, c_long, *mut t_atom) -> *mut c_void,
                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            >(gimme)),
            c"gimme".as_ptr(),
            ty::A_GIMME,
            0,
        );
        let x = sys::object_alloc(c);
        assert_eq!(
            sys::object_method(x.cast(), sys::gensym(c"zero".as_ptr())),
            x.cast()
        );
        let symbol = sys::gensym(c"value".as_ptr());
        let payload = std::ptr::dangling_mut::<c_void>();
        let large = (1i64 << 40) as t_atom_long;
        assert_eq!(
            sys::object_method(
                x.cast(),
                sys::gensym(c"mixed".as_ptr()),
                -large,
                2.75f64,
                symbol,
                payload,
                large
            ),
            payload
        );
        let mut atom: t_atom = std::mem::zeroed();
        let atoms = &mut atom as *mut t_atom;
        assert_eq!(
            sys::object_method(
                x.cast(),
                sys::gensym(c"gimme".as_ptr()),
                symbol,
                1 as c_long,
                atoms
            ),
            atoms.cast()
        );
        assert_eq!(
            *CALLS.lock().unwrap(),
            [
                (-large, 2.75, symbol as usize, payload as usize, large),
                (1, 0., symbol as usize, atoms as usize, 0),
            ]
        );
        assert!(k.take_console_log().is_empty());
        // No variadic args are provided: these paths must refuse before reading any.
        assert!(sys::object_method(x.cast(), sys::gensym(c"untyped".as_ptr())).is_null());
        assert!(sys::object_method(x.cast(), sys::gensym(c"missing".as_ptr())).is_null());
        let errors = k.take_console_log();
        assert_eq!(errors.len(), 2);
        assert!(
            errors[0]
                .text
                .contains("A_CANT requires a direct typed call")
        );
        assert!(errors[1].text.contains("no method"));
        assert!(k.free_object(x.cast()));
    }
}
