//! A port of the SDK's `basics/dummy`: stores and outputs the last int, float or selector it got,
//! with a `name` attribute, a hidden and a revealed method, and an `A_CANT` method marked
//! documentable.
//!
//! The name argument is the first positional argument, so `@name foo` sets the attribute (the
//! SDK takes the first atom whatever it is and never processes attribute arguments).
//! Untested: the unique name given when there is no name argument (`median-mock` has no
//! `symbol_unique`).

use std::{
    ffi::{CStr, c_int, c_void},
    sync::Mutex,
};

use median::{max_sys, prelude::*};

#[derive(Clone, Copy)]
enum Value {
    Int(t_atom_long),
    Float(f64),
    Sym(SymbolRef),
}

#[derive(MaxObj)]
#[max(name = "median.dummy")]
pub struct Dummy {
    obj: ObjRef,
    val: Mutex<Value>,
    name: Mutex<SymbolRef>,
    out: Outlet<Any>,
}

impl MaxObjWrapped for Dummy {
    fn class_setup(class: &mut Class<MaxObjWrapper<Self>>) {
        let c = class.inner();
        // GAP: A_CANT method. median registers only its own fixed set of `A_CANT` methods.
        // SAFETY: `blooop` takes the object and nothing else, as an `A_CANT` method with no
        // arguments is called; the class is the live class being set up.
        unsafe {
            let m = std::mem::transmute::<
                unsafe extern "C" fn(*mut MaxObjWrapper<Dummy>),
                unsafe extern "C" fn(*mut c_void) -> *mut c_void,
            >(blooop);
            max_sys::class_addmethod(
                c,
                Some(m),
                c"blooop".as_ptr(),
                max_sys::e_max_atomtypes::A_CANT as c_int,
                0,
            );
        }
        // GAP: CLASS_METHOD_ATTR_PARSE.
        // SAFETY: the class is live and has both methods.
        unsafe {
            method_attr(c, c"identify", c"undocumented");
            method_attr(c, c"blooop", c"documentable");
        }
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let name = builder
            .args()
            .positional()
            .first()
            .map(|a| a.get_symbol())
            .filter(|s| !s.is_empty())
            // GAP: symbol_unique.
            // SAFETY: `symbol_unique` returns an interned symbol.
            .unwrap_or_else(|| unsafe { SymbolRef::from_raw(max_sys::symbol_unique()) });
        builder.inlet(Inlet::default().assist("I am inlet 0"));
        Ok(Self {
            obj: builder.obj(),
            val: Mutex::new(Value::Int(0)),
            name: Mutex::new(name),
            out: builder.outlet(Outlet::any().assist("I am outlet 0")),
        })
    }
}

/// # Safety
/// `x` must be a live `median.dummy` object.
unsafe extern "C" fn blooop(x: *mut MaxObjWrapper<Dummy>) {
    // SAFETY: the caller's contract
    let w = unsafe { &*x };
    median::ffi::guard(w, "blooop", || w.wrapped().obj.post("can't touch this!"));
}

/// Gives the method `name` of `class` the attribute `attr` with the value 1.
///
/// # Safety
/// `class` must be a live class that has the method `name`.
unsafe fn method_attr(class: *mut max_sys::t_class, name: &CStr, attr: &CStr) {
    // SAFETY: the caller's contract; the strings are NUL-terminated
    unsafe {
        let methods = max_sys::class_extra_lookup(class, max_sys::gensym(c"methods".as_ptr()));
        if methods.is_null() {
            return;
        }
        let mut m: *mut max_sys::t_object = std::ptr::null_mut();
        max_sys::hashtab_lookup(methods.cast(), max_sys::gensym(name.as_ptr()), &mut m);
        if !m.is_null() {
            max_sys::object_addattr_parse(
                m,
                attr.as_ptr(),
                max_sys::gensym(c"long".as_ptr()),
                0,
                c"1".as_ptr(),
            );
        }
    }
}

#[median::methods]
impl Dummy {
    #[bang]
    pub fn bang(&self) {
        let _ = match *self.val.lock().unwrap() {
            Value::Int(i) => self.out.send_int(i),
            Value::Float(f) => self.out.send_float(f),
            Value::Sym(s) => self.out.send(&s, &[]),
        };
    }

    #[int]
    pub fn int(&self, n: t_atom_long) {
        *self.val.lock().unwrap() = Value::Int(n);
        self.bang();
    }

    #[float]
    pub fn float(&self, f: f64) {
        *self.val.lock().unwrap() = Value::Float(f);
        self.bang();
    }

    #[any]
    pub fn anything(&self, sel: &SymbolRef, _atoms: &[Atom]) {
        *self.val.lock().unwrap() = Value::Sym(*sel);
        self.bang();
    }

    #[gimme("xyzzy")]
    pub fn xyzzy(&self, _atoms: &[Atom]) {
        self.obj.post("A hollow voice says 'Plugh'");
    }

    #[sel("identify")]
    pub fn identify(&self) {
        median::object_post!(&self.obj, "my name is {}", self.name());
    }

    #[dblclick]
    pub fn dblclick(&self) {
        self.obj.post("I got a double-click");
    }

    #[attr]
    pub fn name(&self) -> SymbolRef {
        *self.name.lock().unwrap()
    }

    pub fn set_name(&self, name: SymbolRef) {
        *self.name.lock().unwrap() = name;
    }
}

median::ext_main! { Dummy }

#[cfg(test)]
mod tests {
    use median_mock::Atom;
    use std::ffi::c_void;

    #[test]
    fn outputs_the_last_value() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.dummy", &[Atom::Sym("fred".into())]);
        assert!(!x.is_null());
        k.send_bang(x).unwrap();
        k.send_int(x, 3).unwrap();
        k.send_float(x, 1.5).unwrap();
        k.send(x, "hello", &[Atom::Long(1)]).unwrap();
        k.send(x, "xyzzy", &[]).unwrap();
        k.send_bang(x).unwrap();
        let out: Vec<_> = k
            .take_outlet_log()
            .into_iter()
            .map(|e| (e.selector, e.atoms))
            .collect();
        assert_eq!(
            out,
            [
                ("int".into(), vec![Atom::Long(0)]),
                ("int".into(), vec![Atom::Long(3)]),
                ("float".into(), vec![Atom::Float(1.5)]),
                ("hello".into(), vec![]),
                ("hello".into(), vec![]),
            ]
        );
        k.send(x, "identify", &[]).unwrap();
        k.attr_set(x, "name", &[Atom::Sym("barney".into())])
            .unwrap();
        k.send(x, "identify", &[]).unwrap();
        assert!(k.dblclick(x));
        let console: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(
            console,
            [
                "A hollow voice says 'Plugh'",
                "my name is fred",
                "my name is barney",
                "I got a double-click",
            ]
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn method_attributes_and_a_cant() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.dummy", &[Atom::Sym("fred".into())]);
        assert_eq!(
            k.method_attr(x, "identify", "undocumented"),
            Some(("long".into(), "1".into()))
        );
        assert_eq!(
            k.method_attr(x, "blooop", "documentable"),
            Some(("long".into(), "1".into()))
        );
        assert!(k.send(x, "blooop", &[]).is_err());
        let (f, types) = k.method(x, "blooop").unwrap();
        assert_eq!(types, [median::max_sys::e_max_atomtypes::A_CANT as i32]);
        // SAFETY: `blooop` is `A_CANT` with no arguments: it takes the object only.
        unsafe {
            let f: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(f);
            f(x);
        }
        assert_eq!(k.take_console_log()[0].text, "can't touch this!");
        assert!(k.free_object(x));
    }
}
