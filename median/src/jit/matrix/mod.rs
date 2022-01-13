//! Jitter Matrix Operator

use super::{Class, CLASSES};
use crate::{method::MaxMethod, symbol::SymbolRef};
use max_sys::{t_jit_err, t_jit_matrix_info, t_jit_object};

use std::{
    ffi::{c_void, CString},
    marker::PhantomData,
    mem::MaybeUninit,
    os::raw::{c_char, c_long},
};

pub mod iter;
pub mod parallel;

pub const JIT_MATRIX_MAX_PLANECOUNT: usize = 32;
pub const JIT_MATRIX_MAX_DIMCOUNT: usize = 32;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Count {
    Variable,
    Fixed(usize),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MatrixSize {
    Unknown,
    Bytes(usize),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct IOCount {
    pub inputs: Count,
    pub outputs: Count,
}

pub struct Matrix {
    inner: *mut c_void,
}

pub struct MatrixGuard<'a> {
    matrix: &'a Matrix,
    lock: c_long,
}

#[repr(transparent)]
#[derive(Clone)]
pub struct MatrixInfo {
    inner: t_jit_matrix_info,
}

pub struct MatrixData<'a> {
    inner: *mut c_char,
    _phantom: PhantomData<&'a ()>,
}

/// Trait for a Wrapped Matrix operator
pub trait WrappedMatrixOp: Sync + Send {
    type Wrapper;

    /// Creation
    fn new() -> Self;

    /// The name of your jitter class.
    fn class_name() -> &'static str;

    /// Setup your class after creation, before registration
    fn class_setup(_class: &Class) {}

    /// Get the matrix IO Counts
    fn io_count() -> IOCount;

    /// Setup your MOP before adornment
    fn mop_setup(_mop: *mut t_jit_object) {}

    /// Calculate your matrices
    fn calc(
        &self,
        inputs: &[Matrix],
        outputs: &[Matrix],
    ) -> Result<(), max_sys::t_jit_error_code::Type>;
}

struct WrapperInner<T> {
    wrapped: T,
    inputs: Vec<Matrix>,
    outputs: Vec<Matrix>,
}

/// A struct that wraps a `WrappedMatrixOp` and makes it into a jitter object.
#[repr(C)]
pub struct Wrapper<T> {
    ob: max_sys::t_jit_object,
    inner: MaybeUninit<WrapperInner<T>>,
}

impl Matrix {
    pub fn inner(&self) -> *mut c_void {
        self.inner
    }

    fn new(inner: *mut c_void) -> Self {
        Self { inner }
    }

    pub fn lock(&self) -> MatrixGuard<'_> {
        let lock =
            unsafe { max_sys::jit_object_method(self.inner, max_sys::_jit_sym_lock, 1) as _ };
        MatrixGuard { matrix: self, lock }
    }
}

impl<'a> MatrixGuard<'a> {
    /// Get the Matrix info
    pub fn info(&self) -> MatrixInfo {
        unsafe {
            let mut info: MaybeUninit<t_jit_matrix_info> = MaybeUninit::uninit();
            max_sys::jit_object_method(
                self.matrix.inner(),
                max_sys::_jit_sym_getinfo,
                info.as_mut_ptr(),
            );
            MatrixInfo {
                inner: info.assume_init(),
            }
        }
    }

    /// Set/overwrite the info for this matrix
    pub fn set_info(&mut self, info: MatrixInfo) {
        unsafe {
            max_sys::jit_object_method(
                self.matrix.inner(),
                max_sys::_jit_sym_setinfo_ex,
                info.inner(),
            );
        }
    }

    /// Get the Matrix Data, if there is any
    pub fn data(&mut self) -> Option<MatrixData<'_>> {
        unsafe {
            let mut p: MaybeUninit<*mut c_char> = MaybeUninit::zeroed();
            max_sys::jit_object_method(
                self.matrix.inner(),
                max_sys::_jit_sym_getdata,
                p.as_mut_ptr(),
            );

            let inner = p.assume_init();
            if inner.is_null() {
                None
            } else {
                Some(MatrixData {
                    inner,
                    _phantom: Default::default(),
                })
            }
        }
    }

    /// Set the data for this matrix
    pub fn set_data(&mut self, data: &MatrixData<'_>) {
        unsafe {
            max_sys::jit_object_method(self.matrix.inner(), max_sys::_jit_sym_data, data.inner());
        }
    }
}

impl<'a> Drop for MatrixGuard<'a> {
    fn drop(&mut self) {
        unsafe {
            max_sys::jit_object_method(self.matrix.inner(), max_sys::_jit_sym_lock, self.lock);
        }
    }
}

impl<'a> MatrixData<'a> {
    unsafe fn new(inner: *mut c_char) -> Self {
        Self {
            inner,
            _phantom: Default::default(),
        }
    }

    /// Get a raw pointer to the matrix data.
    pub fn inner(&self) -> *mut c_char {
        self.inner
    }
}

impl MatrixInfo {
    /// Size if known
    pub fn size(&self) -> MatrixSize {
        let s = self.inner.size;
        if s == 0xFFFFFFFF {
            MatrixSize::Unknown
        } else {
            assert!(s >= 0);
            MatrixSize::Bytes(s as _)
        }
    }

    /// Primitive type (char, long, float32, or float64)
    pub fn primitive_type(&self) -> SymbolRef {
        SymbolRef::from(self.inner.type_)
    }

    /// Is the primitive type a char
    pub fn is_char(&self) -> bool {
        unsafe { self.inner.type_ == max_sys::_jit_sym_char }
    }

    /// Set the primitive type to long
    pub fn set_char(&mut self) {
        unsafe {
            self.inner.type_ = max_sys::_jit_sym_char;
        }
    }

    /// Is the primitive type a long
    pub fn is_long(&self) -> bool {
        unsafe { self.inner.type_ == max_sys::_jit_sym_long }
    }

    /// Set the primitive type to long
    pub fn set_long(&mut self) {
        unsafe {
            self.inner.type_ = max_sys::_jit_sym_long;
        }
    }

    /// Is the primitive type a float32
    pub fn is_f32(&self) -> bool {
        unsafe { self.inner.type_ == max_sys::_jit_sym_float32 }
    }

    /// Set the primitive type to float32
    pub fn set_f32(&mut self) {
        unsafe {
            self.inner.type_ = max_sys::_jit_sym_float32;
        }
    }

    /// Is the primitive type a float64
    pub fn is_f64(&self) -> bool {
        unsafe { self.inner.type_ == max_sys::_jit_sym_float64 }
    }

    /// Set the primitive type to float64
    pub fn set_f64(&mut self) {
        unsafe {
            self.inner.type_ = max_sys::_jit_sym_float64;
        }
    }

    /// Flags to specify data reference, handle, or tightly packed
    pub fn flags(&self) -> c_long {
        self.inner.flags
    }

    /// Set the flags
    pub fn set_flags(&mut self, v: c_long) {
        self.inner.flags = v;
    }

    /// Number of dimensions
    pub fn dim_count(&self) -> usize {
        assert!(self.inner.dimcount >= 0);
        self.inner.dimcount as _
    }

    /// Se the number of dimensions
    pub fn set_dim_count(&mut self, v: usize) {
        self.inner.dimcount = v as _;
    }

    /// Dimension sizes
    pub fn dim_sizes(&self) -> &[c_long; 32] {
        &self.inner.dim
    }

    /// Get a mutable reference to the Dimension sizes
    pub fn dim_sizes_mut(&mut self) -> &mut [c_long; 32] {
        &mut self.inner.dim
    }

    /// Stride across dimensions in bytes
    pub fn dim_strides(&self) -> &[c_long; 32] {
        &self.inner.dimstride
    }

    /// Number of planes
    pub fn plane_count(&self) -> usize {
        assert!(self.inner.planecount >= 0);
        self.inner.planecount as _
    }

    /// Set the number of planes
    pub fn set_plane_count(&mut self, v: usize) {
        self.inner.planecount = v as _;
    }

    /// Get a raw pointer to the inner `max_sys::t_jit_matrix_info` struct
    pub fn inner(&self) -> *const t_jit_matrix_info {
        &self.inner as _
    }
}

impl Default for MatrixInfo {
    fn default() -> Self {
        let mut inner = MaybeUninit::zeroed();
        unsafe {
            max_sys::jit_matrix_info_default(inner.as_mut_ptr());
            Self {
                inner: inner.assume_init(),
            }
        }
    }
}

impl<T> Wrapper<T>
where
    T: WrappedMatrixOp<Wrapper = Self>,
{
    /// The wrapper initialization method
    pub unsafe fn init() -> max_sys::t_jit_err {
        let name = CString::new(T::class_name()).expect("couldn't convert name to CString");
        let class = max_sys::jit_class_new(
            name.as_ptr(),
            Some(std::mem::transmute::<
                unsafe extern "C" fn() -> *mut Self,
                MaxMethod,
            >(Self::new)),
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut Self),
                MaxMethod,
            >(Self::free)),
            std::mem::size_of::<Self>() as c_long,
            0,
        );

        let (inputs, outputs): (c_long, c_long) = T::io_count().into();
        let mop = max_sys::jit_object_new(max_sys::_jit_sym_jit_mop, inputs, outputs) as _;
        T::mop_setup(mop);
        max_sys::jit_class_addadornment(class, mop);

        let name = CString::new("matrix_calc").unwrap();
        max_sys::jit_class_addmethod(
            class,
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut Self, *mut c_void, *mut c_void) -> max_sys::t_jit_err,
                MaxMethod,
            >(Self::calc)),
            name.as_ptr(),
            max_sys::e_max_atomtypes::A_CANT as c_long,
            0,
        );

        let class = Class { inner: class };
        T::class_setup(&class);

        max_sys::jit_class_register(class.inner);

        let mut g = CLASSES.lock().expect("couldn't lock CLASSES mutex");
        g.insert(T::class_name(), class);

        max_sys::t_jit_error_code::JIT_ERR_NONE as _
    }

    /// The object creation method to register with max
    unsafe extern "C" fn new() -> *mut Self {
        let g = CLASSES.lock().expect("couldn't lock CLASSES mutex");
        let c = g.get(T::class_name()).expect("couldn't find class by name");

        let x = max_sys::jit_object_alloc(c.inner);
        if !x.is_null() {
            //initialize
            let x: &mut Self = std::mem::transmute(x as *mut Self);
            x.inner = MaybeUninit::new(WrapperInner::<T>::new());
        }

        let x: *mut Self = x as _;
        //make sure our offset computation is correct
        assert_eq!(x as *mut t_jit_object, Self::inner(x).wrapped().jit_obj());

        x
    }

    /// The object free method to register with max
    unsafe extern "C" fn free(x: *mut Self) {
        let x: &mut Self = &mut *x;
        //free wrapped
        let mut inner = MaybeUninit::uninit();
        std::mem::swap(&mut x.inner, &mut inner);
        std::mem::drop(inner.assume_init());
    }

    unsafe fn inner<'a>(x: *mut Self) -> &'a mut WrapperInner<T> {
        let x: &mut Self = std::mem::transmute(x as *mut Self);
        &mut *x.inner.as_mut_ptr()
    }

    /// Get a reference to the wrapped object, useful for trampolines
    pub unsafe fn wrapped<'a>(x: *mut Self) -> &'a T {
        Self::inner(x).wrapped()
    }

    /// The matrix calculation method to register with max
    unsafe extern "C" fn calc(
        x: *mut Self,
        inputs: *mut c_void,
        outputs: *mut c_void,
    ) -> max_sys::t_jit_err {
        Self::inner(x).calc(inputs, outputs)
    }
}

impl<T> WrapperInner<T>
where
    T: WrappedMatrixOp<Wrapper = Wrapper<T>>,
{
    fn wrapped(&self) -> &T {
        &self.wrapped
    }

    fn new() -> Self {
        let IOCount { inputs, outputs } = T::io_count();
        let inputs = Vec::with_capacity(match inputs {
            Count::Fixed(s) => s,
            Count::Variable => JIT_MATRIX_MAX_PLANECOUNT,
        });
        let outputs = Vec::with_capacity(match outputs {
            Count::Fixed(s) => s,
            Count::Variable => JIT_MATRIX_MAX_PLANECOUNT,
        });
        Self {
            wrapped: T::new(),
            inputs,
            outputs,
        }
    }

    //XXX should we lock the vectors?? can this be called from multiple threads?
    fn calc(&mut self, inputs: *mut c_void, outputs: *mut c_void) -> t_jit_err {
        unsafe {
            //populate input and output vectors
            let fill = |v: *mut c_void, o: &mut Vec<Matrix>| -> Result<(), t_jit_err> {
                o.clear();

                //TODO optimize if we already know the number of matrices?
                let c: c_long = max_sys::jit_object_method(v, max_sys::_jit_sym_getsize) as _;
                assert!(c > 0);
                for i in 0..c {
                    let p = max_sys::jit_object_method(v, max_sys::_jit_sym_getindex, i);
                    //TODO do we need to do this if we've just looked up the size?
                    if p == std::ptr::null_mut() {
                        return Err(max_sys::t_jit_error_code::JIT_ERR_INVALID_PTR as _);
                    } else {
                        o.push(Matrix::new(p));
                    }
                }
                Ok(())
            };
            if let Err(e) = fill(inputs, &mut self.inputs) {
                return e;
            }
            if let Err(e) = fill(outputs, &mut self.outputs) {
                return e;
            }
        }

        match self
            .wrapped
            .calc(self.inputs.as_slice(), self.outputs.as_slice())
        {
            Ok(()) => max_sys::t_jit_error_code::JIT_ERR_NONE as _,
            Err(e) => e as _,
        }
    }
}

impl Into<c_long> for Count {
    fn into(self) -> c_long {
        match self {
            Self::Variable => -1,
            Self::Fixed(count) => count as _,
        }
    }
}

impl Into<(c_long, c_long)> for IOCount {
    fn into(self) -> (c_long, c_long) {
        (self.inputs.into(), self.outputs.into())
    }
}

/// Trait for getting the jitter object, largely useful in a wrapped object
pub unsafe trait JitObj: Sized {
    /// Get the jitter object pointer
    fn jit_obj(&self) -> *mut max_sys::t_jit_object;

    /// Initialize/register your jitter object.
    fn init();
}

unsafe impl<T> JitObj for T
where
    T: WrappedMatrixOp<Wrapper = Wrapper<T>>,
{
    fn jit_obj(&self) -> *mut max_sys::t_jit_object {
        let off = field_offset::offset_of!(Wrapper<T> => inner).get_byte_offset()
            + field_offset::offset_of!(WrapperInner<T> => wrapped).get_byte_offset();

        unsafe {
            let ptr: *mut u8 = std::mem::transmute::<_, *mut u8>(self as *const T);
            std::mem::transmute::<_, *mut max_sys::t_jit_object>(ptr.offset(-(off as isize)))
        }
    }

    fn init() {
        unsafe {
            Wrapper::<T>::init();
        }
    }
}
