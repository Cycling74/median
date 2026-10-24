//! CPU chunk/descriptor fixtures for GL ownership tests; no OpenGL calls are made.
use super::*;
use core::ffi::c_int;
use std::{
    collections::HashSet,
    sync::atomic::{AtomicU8, AtomicUsize, Ordering},
};
static CHUNKS: LazyLock<parking_lot::Mutex<HashSet<usize>>> = LazyLock::new(Default::default);
static FAIL_COPY: AtomicU8 = AtomicU8::new(0);
static BUFFER_DESTROYS: AtomicUsize = AtomicUsize::new(0);
/// A mock-only owned geometry, freed by `jit_object_free`; no GPU resources or uploads.
pub fn new_geometry() -> *mut c_void {
    alloc(Aux::Geometry)
}
pub fn live_geometry_count() -> usize {
    JIT.lock()
        .aux
        .values()
        .filter(|a| matches!(a, Aux::Geometry))
        .count()
}
/// A mock-only descriptor with no allocated views. Payload ownership stays with the caller.
/// Destruction only clears the pointer, and is counted by `buffer_destroy_count`.
pub fn buffer_descriptor(payload: &mut [u8]) -> t_jit_gl_buffer_data {
    t_jit_gl_buffer_data {
        ptr: payload.as_mut_ptr().cast(),
        size: payload.len() as c_long,
        ..unsafe { std::mem::zeroed() }
    }
}
pub fn buffer_destroy_count() -> usize {
    BUFFER_DESTROYS.load(Ordering::SeqCst)
}
pub fn live_chunk_count() -> usize {
    CHUNKS.lock().len()
}
pub fn fail_next_chunk_copy(partial: bool) {
    FAIL_COPY.store(if partial { 2 } else { 1 }, Ordering::SeqCst);
}
pub(super) fn reset() {
    let chunks: Vec<_> = CHUNKS.lock().iter().copied().collect();
    for ptr in chunks {
        unsafe {
            jit_glchunk_delete(ptr as _);
        }
    }
    FAIL_COPY.store(0, Ordering::SeqCst);
    BUFFER_DESTROYS.store(0, Ordering::SeqCst);
}
fn matrix(planes: c_int, dimensions: &[c_int], int: bool) -> *mut t_object {
    let mut info: t_jit_matrix_info = unsafe { std::mem::zeroed() };
    info.type_ = intern(if int { b"long" } else { b"float32" });
    info.planecount = planes as _;
    info.dimcount = dimensions.len() as _;
    let mut size = planes as c_long * 4;
    for (i, &dim) in dimensions.iter().enumerate() {
        info.dim[i] = dim as _;
        info.dimstride[i] = size;
        size *= dim as c_long;
    }
    info.size = size;
    alloc(Aux::Matrix(Box::new(MatrixData {
        info,
        data: vec![0; (size as usize).div_ceil(8)],
        locked: 0,
    })))
    .cast()
}
fn chunk(
    primitive: *mut t_symbol,
    vertex: *mut t_object,
    index: *mut t_object,
) -> *mut t_jit_glchunk {
    let mut result: t_jit_glchunk = unsafe { std::mem::zeroed() };
    result.prim = primitive;
    result.m_vertex = vertex;
    result.m_index = index;
    let ptr = Box::into_raw(Box::new(result));
    CHUNKS.lock().insert(ptr as usize);
    ptr
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_glchunk_new(
    primitive: *mut t_symbol,
    planes: c_int,
    vertices: c_int,
    indices: c_int,
) -> *mut t_jit_glchunk {
    chunk(
        primitive,
        matrix(planes, &[vertices], false),
        if indices == 0 {
            std::ptr::null_mut()
        } else {
            matrix(1, &[indices], true)
        },
    )
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_glchunk_grid_new(
    primitive: *mut t_symbol,
    planes: c_int,
    width: c_int,
    height: c_int,
) -> *mut t_jit_glchunk {
    chunk(
        primitive,
        matrix(planes, &[width, height], false),
        std::ptr::null_mut(),
    )
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_glchunk_delete(ptr: *mut t_jit_glchunk) {
    assert!(
        CHUNKS.lock().remove(&(ptr as usize)),
        "chunk must be deleted once"
    );
    let value = unsafe { Box::from_raw(ptr) };
    unsafe {
        jit_object_free(value.m_vertex.cast());
        if !value.m_index.is_null() {
            jit_object_free(value.m_index.cast());
        }
    }
}
fn copy_matrix(ptr: *mut t_object) -> *mut t_object {
    if ptr.is_null() {
        return ptr;
    }
    let copy = {
        let jit = JIT.lock();
        let Aux::Matrix(matrix) = &jit.aux[&(ptr as usize)] else {
            panic!("live matrix");
        };
        MatrixData {
            info: matrix.info,
            data: matrix.data.clone(),
            locked: 0,
        }
    };
    alloc(Aux::Matrix(Box::new(copy))).cast()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_glchunk_copy(
    out: *mut *mut t_jit_glchunk,
    source: *mut t_jit_glchunk,
) -> t_jit_err {
    let failure = FAIL_COPY.swap(0, Ordering::SeqCst);
    if failure == 1 {
        return t_jit_error_code::JIT_ERR_OUT_OF_MEM as _;
    }
    let source = unsafe { &*source };
    let ptr = chunk(
        source.prim,
        copy_matrix(source.m_vertex),
        copy_matrix(source.m_index),
    );
    unsafe {
        out.write(ptr);
    }
    if failure == 2 {
        t_jit_error_code::JIT_ERR_OUT_OF_MEM as _
    } else {
        0
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_ob3d_get(owner: *mut c_void) -> *mut c_void {
    JIT.lock()
        .contexts
        .get(&(owner as usize))
        .copied()
        .unwrap_or(0) as _
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_ob3d_draw_chunk(
    ob3d: *mut c_void,
    ptr: *mut t_jit_glchunk,
) -> t_jit_err {
    assert!(CHUNKS.lock().contains(&(ptr as usize)));
    if JIT.lock().contexts.values().any(|&p| p == ob3d as usize) {
        0
    } else {
        t_jit_error_code::JIT_ERR_INVALID_PTR as _
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_ob3d_draw_geometry(
    ob3d: *mut c_void,
    geometry: *mut t_object,
) -> t_jit_err {
    let jit = JIT.lock();
    if jit.contexts.values().any(|&p| p == ob3d as usize)
        && matches!(jit.aux.get(&(geometry as usize)), Some(Aux::Geometry))
    {
        0
    } else {
        t_jit_error_code::JIT_ERR_INVALID_PTR as _
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jit_gl_buffer_data_destroy_tagged(data: *mut t_jit_gl_buffer_data) {
    // Fixture descriptors have zero views and borrowed payloads. Do not model undocumented
    // SDK allocation details: tests explicitly adopt this descriptor under its unsafe contract.
    let data = unsafe { &mut *data };
    assert_eq!(data.nviews, 0);
    assert!(data.views.is_null());
    data.ptr = std::ptr::null_mut();
    BUFFER_DESTROYS.fetch_add(1, Ordering::SeqCst);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunks_own_independent_matrices_and_free_once() {
        let _k = crate::lock();
        unsafe {
            let source = jit_glchunk_new(intern(b"triangles"), 3, 3, 0);
            let mut copy = std::ptr::null_mut();
            assert_eq!(jit_glchunk_copy(&mut copy, source), 0);
            assert_ne!((*copy).m_vertex, (*source).m_vertex);
            jit_glchunk_delete(source);
            jit_glchunk_delete(copy);
            assert_eq!(live_chunk_count(), 0);
        }
    }
}
