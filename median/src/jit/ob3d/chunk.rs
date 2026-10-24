use crate::{
    jit::{DrawContext, JitResult, matrix::Matrix},
    symbol::SymbolRef,
};
use core::ffi::c_int;
use max_sys::{self as sys, t_jit_error_code as err};
use std::ptr::NonNull;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GLPrim {
    Points,
    Lines,
    LineStrip,
    LineLoop,
    Triangles,
    TriangleStrip,
    TriangleFan,
    Quads,
    QuadStrip,
    Polygon,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GLGridPrim {
    Quad,
    Triangle,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GLChunkPrim {
    Prim(GLPrim),
    Grid(GLGridPrim),
}
macro_rules! symbols {
    ($ty:ident { $($variant:ident => $name:literal),+ $(,)? }) => {
        impl From<$ty> for SymbolRef {
            fn from(value: $ty) -> Self { match value { $($ty::$variant => crate::sym!($name)),+ } }
        }
        impl TryFrom<SymbolRef> for $ty {
            type Error = crate::jit::JitError;
            fn try_from(value: SymbolRef) -> JitResult<Self> {
                $(if value == crate::sym!($name) { return Ok(Self::$variant); })+
                Err(err::JIT_ERR_INVALID_INPUT)
            }
        }
    };
}
symbols!(GLPrim {
    Points => "points", Lines => "lines", LineStrip => "line_strip", LineLoop => "line_loop",
    Triangles => "triangles", TriangleStrip => "tri_strip", TriangleFan => "tri_fan",
    Quads => "quads", QuadStrip => "quad_strip", Polygon => "polygon",
});
symbols!(GLGridPrim { Quad => "quad_grid", Triangle => "tri_grid" });
impl TryFrom<SymbolRef> for GLChunkPrim {
    type Error = crate::jit::JitError;
    fn try_from(value: SymbolRef) -> JitResult<Self> {
        GLPrim::try_from(value)
            .map(Self::Prim)
            .or_else(|_| GLGridPrim::try_from(value).map(Self::Grid))
    }
}

/// Own one SDK GL chunk and its matrices, bounded by a host callback context.
/// No Rust slice aliasing guarantee follows from ownership: the matrices can be registered
/// with Jitter, so matrix-data access still requires its explicit unsafe contract.
pub struct GLChunk<'ctx> {
    ptr: NonNull<sys::t_jit_glchunk>,
    context: &'ctx DrawContext,
}
fn dimensions(planes: usize, vertices: usize, indices: usize) -> JitResult<(c_int, c_int, c_int)> {
    if !(1..=32).contains(&planes) || vertices == 0 {
        return Err(err::JIT_ERR_INVALID_INPUT);
    }
    // Bound intermediate int arithmetic used by SDK allocators, on Windows as well as macOS.
    if planes
        .checked_mul(vertices)
        .and_then(|n| n.checked_mul(4))
        .is_none_or(|n| n > c_int::MAX as usize)
        || indices > c_int::MAX as usize / 4
    {
        return Err(err::JIT_ERR_OUT_OF_BOUNDS);
    }
    Ok((planes as _, vertices as _, indices as _))
}
impl<'ctx> GLChunk<'ctx> {
    pub fn new(
        context: &'ctx DrawContext,
        primitive: GLPrim,
        planes: usize,
        vertices: usize,
        indices: Option<usize>,
    ) -> JitResult<Self> {
        let (p, v, i) = dimensions(planes, vertices, indices.unwrap_or(0))?;
        unsafe {
            Self::from_raw(
                context,
                sys::jit_glchunk_new(SymbolRef::from(primitive).as_ptr(), p, v, i),
            )
        }
    }
    pub fn new_grid(
        context: &'ctx DrawContext,
        primitive: GLGridPrim,
        planes: usize,
        width: usize,
        height: usize,
    ) -> JitResult<Self> {
        let vertices = width
            .checked_mul(height)
            .ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?;
        dimensions(planes, vertices, 0)?;
        // Grid triangulation can require up to six indices per cell.
        let indices = vertices.checked_mul(6).ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?;
        dimensions(planes, vertices, indices)?;
        unsafe {
            Self::from_raw(
                context,
                sys::jit_glchunk_grid_new(
                    SymbolRef::from(primitive).as_ptr(),
                    planes as _,
                    width as _,
                    height as _,
                ),
            )
        }
    }
    /// Adopt a chunk that must be deleted exactly once in this callback's context.
    ///
    /// # Safety
    /// A non-null pointer must be a live, exclusively owned SDK chunk with valid matrices and
    /// chain pointers. No other code may delete/modify its descriptor until this owner drops.
    /// Its resources must be compatible with `context`; do not close that context early.
    pub unsafe fn from_raw(
        context: &'ctx DrawContext,
        ptr: *mut sys::t_jit_glchunk,
    ) -> JitResult<Self> {
        Ok(Self {
            ptr: NonNull::new(ptr).ok_or(err::JIT_ERR_OUT_OF_MEM)?,
            context,
        })
    }
    pub fn as_ptr(&self) -> *mut sys::t_jit_glchunk {
        self.ptr.as_ptr()
    }
    pub fn primitive(&self) -> JitResult<GLChunkPrim> {
        unsafe { SymbolRef::from_raw((*self.as_ptr()).prim) }.try_into()
    }
    pub fn try_clone(&self) -> JitResult<Self> {
        let mut ptr = std::ptr::null_mut();
        let code = unsafe { sys::jit_glchunk_copy(&mut ptr, self.as_ptr()) };
        if code != 0 {
            if !ptr.is_null() {
                unsafe {
                    sys::jit_glchunk_delete(ptr);
                }
            }
            return Err(code as _);
        }
        unsafe { Self::from_raw(self.context, ptr) }
    }
    pub fn vertices(&mut self) -> JitResult<Matrix<'_>> {
        unsafe { Matrix::from_raw((*self.as_ptr()).m_vertex.cast()) }
    }
    pub fn indices(&mut self) -> JitResult<Option<Matrix<'_>>> {
        let ptr = unsafe { (*self.as_ptr()).m_index };
        if ptr.is_null() {
            Ok(None)
        } else {
            unsafe { Matrix::from_raw(ptr.cast()) }.map(Some)
        }
    }
    /// Draw through the ob3d that supplied this callback context.
    ///
    /// # Safety
    /// Vertex/index data must be initialized, stable and valid for the primitive; indices must
    /// refer to existing vertices. No other code may mutate/delete their storage during drawing.
    pub unsafe fn draw(&self) -> JitResult<()> {
        let code = unsafe { sys::jit_ob3d_draw_chunk(self.context.ob3d(), self.as_ptr()) };
        if code == 0 { Ok(()) } else { Err(code as _) }
    }
}
impl Drop for GLChunk<'_> {
    fn drop(&mut self) {
        unsafe {
            sys::jit_glchunk_delete(self.as_ptr());
        }
    }
}
