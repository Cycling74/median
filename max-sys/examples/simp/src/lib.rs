use std::ffi::c_void;
use std::ffi::CString;

static mut SIMP_CLASS: Option<*mut max_sys::t_class> = None;

type Method = unsafe extern "C" fn(arg1: *mut c_void, ...) -> *mut c_void;

#[repr(C)]
struct Simp {
    s_obj: max_sys::t_object,
    s_value: i64,
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
        unsafe {
            max_sys::post(
                CString::new("from rust, value is %ld").unwrap().as_ptr(),
                self.s_value,
            );
        }
    }

    pub fn int(&mut self, v: i64) {
        unsafe {
            max_sys::post(CString::new("from rust, value is %ld").unwrap().as_ptr(), v);
        }
        self.s_value = v
    }

    pub unsafe extern "C" fn bang_trampoline(s: *mut Self) {
        let obj = &mut *(s as *mut Self);
        obj.bang();
    }

    pub unsafe extern "C" fn int_trampoline(s: *mut Self, v: i64) {
        let obj = &mut *(s as *mut Self);
        obj.int(v);
    }
}

#[no_mangle]
pub unsafe extern "C" fn ext_main(_r: *mut c_void) {
    let c = max_sys::class_new(
        CString::new("simp").unwrap().as_ptr(),
        Some(std::mem::transmute::<
            unsafe extern "C" fn() -> *mut c_void,
            Method,
        >(Simp::new)),
        None,
        std::mem::size_of::<Simp>() as i64,
        None,
        0,
    );

    max_sys::class_addmethod(
        c,
        Some(std::mem::transmute::<
            unsafe extern "C" fn(s: *mut Simp),
            Method,
        >(Simp::bang_trampoline)),
        CString::new("bang").unwrap().as_ptr(),
        0,
    );

    max_sys::class_addmethod(
        c,
        Some(std::mem::transmute::<
            unsafe extern "C" fn(s: *mut Simp, i64),
            Method,
        >(Simp::int_trampoline)),
        CString::new("int").unwrap().as_ptr(),
        max_sys::e_max_atomtypes::A_LONG,
        0,
    );

    max_sys::class_register(max_sys::gensym(CString::new("box").unwrap().as_ptr()), c);
    SIMP_CLASS = Some(c);
}
