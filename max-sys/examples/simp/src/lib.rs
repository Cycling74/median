use max_sys::t_atom_long as max_long;
use std::ffi::c_void;
use std::ffi::CString;

static mut SIMP_CLASS: Option<*mut max_sys::t_class> = None;

type Method = unsafe extern "C" fn(arg1: *mut c_void) -> *mut c_void;

#[repr(C)]
struct Simp {
    s_obj: max_sys::t_object,
    s_value: max_long,
}

impl Simp {
    pub unsafe extern "C" fn new() -> *mut c_void {
        let x = max_sys::object_alloc(SIMP_CLASS.unwrap());

        //since it is simply a u64, don't think we need MaybeUninit
        let mut x = std::mem::transmute::<_, &mut Simp>(x);
        x.s_value = 0;

        std::mem::transmute::<_, _>(x)
    }

    pub fn bang(&mut self) {
        let m = CString::new("from rust, value is %ld").unwrap();
        unsafe {
            max_sys::post(m.as_ptr(), self.s_value);
        }
    }

    pub fn int(&mut self, v: max_long) {
        let m = CString::new("from rust, value is %ld").unwrap();
        unsafe {
            max_sys::post(m.as_ptr(), v);
        }
        self.s_value = v
    }

    pub unsafe extern "C" fn bang_trampoline(s: *mut Self) {
        let obj = &mut *(s as *mut Self);
        obj.bang();
    }

    pub unsafe extern "C" fn int_trampoline(s: *mut Self, v: max_long) {
        let obj = &mut *(s as *mut Self);
        obj.int(v);
    }
}

#[no_mangle]
pub unsafe extern "C" fn ext_main(_r: *mut c_void) {
    let name = CString::new("simp").unwrap();
    let c = max_sys::class_new(
        name.as_ptr(),
        Some(std::mem::transmute::<
            unsafe extern "C" fn() -> *mut c_void,
            Method,
        >(Simp::new)),
        None,
        std::mem::size_of::<Simp>() as _,
        None,
        0,
    );

    let bang = CString::new("bang").unwrap();
    max_sys::class_addmethod(
        c,
        Some(std::mem::transmute::<
            unsafe extern "C" fn(s: *mut Simp),
            Method,
        >(Simp::bang_trampoline)),
        bang.as_ptr(),
        0,
    );

    let ints = CString::new("int").unwrap();
    max_sys::class_addmethod(
        c,
        Some(std::mem::transmute::<
            unsafe extern "C" fn(s: *mut Simp, max_long),
            Method,
        >(Simp::int_trampoline)),
        ints.as_ptr(),
        max_sys::e_max_atomtypes::A_LONG,
        0,
    );

    let boxs = CString::new("box").unwrap();
    max_sys::class_register(max_sys::gensym(boxs.as_ptr()), c);
    SIMP_CLASS = Some(c);
}
