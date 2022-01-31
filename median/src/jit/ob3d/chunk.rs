use crate::{jit::JitResult, symbol::SymbolRef};
use std::{
    convert::{TryFrom, TryInto},
    mem::MaybeUninit,
};

//chunks are stored with objects so they need to be Send and Sync
unsafe impl Send for GLChunk {}
unsafe impl Sync for GLChunk {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GLPrim {
    QuadStrip,
    TriangleStrip,
    Points,
    Lines,
    LineStrip,
    LineLoop,
    Triangles,
    TriangleFan,
    Quads,
    Polygon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GLGridPrim {
    Quad,
    Triangle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GLChunkPrim {
    Prim(GLPrim),
    Grid(GLGridPrim),
}

/// Wrapper for a GLChunk
#[repr(transparent)]
pub struct GLChunk {
    inner: *mut max_sys::t_jit_glchunk,
}

/*
  TODO are some of these primatives duplicates of others?

    _jit_sym_gl_points 		= gensym("points");
    _jit_sym_gl_point_sprite= gensym("point_sprite");
    _jit_sym_gl_lines 		= gensym("lines");
    _jit_sym_gl_line_strip 	= gensym("line_strip");
    _jit_sym_gl_line_loop 	= gensym("line_loop");
    _jit_sym_gl_triangles 	= gensym("triangles");
    _jit_sym_gl_tri_strip 	= gensym("tri_strip");
    _jit_sym_gl_tri_fan 	= gensym("tri_fan");
    _jit_sym_gl_quads 		= gensym("quads");
    _jit_sym_gl_quad_strip 	= gensym("quad_strip");
    _jit_sym_gl_polygon 	= gensym("polygon");
    _jit_sym_gl_tri_grid 	= gensym("tri_grid");
    _jit_sym_gl_quad_grid 	= gensym("quad_grid");
*/

impl GLChunk {
    /// Get the raw pointer
    pub fn inner(&self) -> *mut max_sys::t_jit_glchunk {
        self.inner
    }

    /// Create a wrapped GLChunk from a raw pointer
    pub unsafe fn from_raw(inner: *mut max_sys::t_jit_glchunk) -> JitResult<Self> {
        if inner.is_null() {
            Err(max_sys::t_jit_error_code::JIT_ERR_GENERIC)
        } else {
            Ok(Self { inner })
        }
    }

    pub fn new(
        prim: GLPrim,
        planes: usize,
        vertices: usize,
        indices: Option<usize>,
    ) -> JitResult<Self> {
        assert_ne!(planes, 0);
        assert_ne!(vertices, 0);
        let sym: SymbolRef = prim.into();
        let inner = unsafe {
            max_sys::jit_glchunk_new(
                sym.inner(),
                planes as _,
                vertices as _,
                indices.unwrap_or(0) as _,
            )
        };
        if inner.is_null() {
            Err(max_sys::t_jit_error_code::JIT_ERR_GENERIC)
        } else {
            Ok(Self { inner })
        }
    }

    pub fn new_grid(
        prim: GLGridPrim,
        planes: usize,
        width: usize,
        height: usize,
    ) -> JitResult<Self> {
        assert_ne!(planes, 0);
        assert_ne!(width, 0);
        assert_ne!(height, 0);
        let sym: SymbolRef = prim.into();
        let inner = unsafe {
            max_sys::jit_glchunk_grid_new(sym.inner(), planes as _, width as _, height as _)
        };
        if inner.is_null() {
            Err(max_sys::t_jit_error_code::JIT_ERR_GENERIC)
        } else {
            Ok(Self { inner })
        }
    }

    pub fn prim(&self) -> JitResult<GLChunkPrim> {
        let prim: SymbolRef = unsafe { SymbolRef::new((*self.inner).prim) };
        prim.try_into()
    }
}

impl Clone for GLChunk {
    fn clone(&self) -> Self {
        let mut inner: MaybeUninit<*mut max_sys::t_jit_glchunk> = MaybeUninit::zeroed();
        let inner = unsafe {
            max_sys::jit_glchunk_copy(inner.as_mut_ptr(), self.inner);
            inner.assume_init()
        };
        assert_ne!(inner, std::ptr::null_mut());
        Self { inner }
    }
}

impl Drop for GLChunk {
    fn drop(&mut self) {
        unsafe {
            max_sys::jit_glchunk_delete(self.inner);
        }
    }
}

impl Into<SymbolRef> for GLPrim {
    fn into(self) -> SymbolRef {
        SymbolRef::new(unsafe {
            match self {
                Self::QuadStrip => max_sys::_jit_sym_gl_quad_strip,
                Self::TriangleStrip => max_sys::_jit_sym_gl_tri_strip,
                Self::Points => max_sys::_jit_sym_gl_points,
                Self::Lines => max_sys::_jit_sym_gl_lines,
                Self::LineStrip => max_sys::_jit_sym_gl_line_strip,
                Self::LineLoop => max_sys::_jit_sym_gl_line_loop,
                Self::Triangles => max_sys::_jit_sym_gl_triangles,
                Self::TriangleFan => max_sys::_jit_sym_gl_tri_fan,
                Self::Quads => max_sys::_jit_sym_gl_quads,
                Self::Polygon => max_sys::_jit_sym_gl_polygon,
            }
        })
    }
}

impl Into<SymbolRef> for GLGridPrim {
    fn into(self) -> SymbolRef {
        SymbolRef::new(unsafe {
            match self {
                Self::Quad => max_sys::_jit_sym_gl_quad_grid,
                Self::Triangle => max_sys::_jit_sym_gl_tri_grid,
            }
        })
    }
}

impl Into<SymbolRef> for GLChunkPrim {
    fn into(self) -> SymbolRef {
        match self {
            Self::Prim(p) => p.into(),
            Self::Grid(p) => p.into(),
        }
    }
}

impl TryFrom<SymbolRef> for GLPrim {
    type Error = max_sys::t_jit_error_code::Type;
    fn try_from(v: SymbolRef) -> Result<Self, Self::Error> {
        unsafe {
            let inner = v.inner();
            if inner == max_sys::_jit_sym_gl_quad_strip {
                Ok(Self::QuadStrip)
            } else if inner == max_sys::_jit_sym_gl_tri_strip {
                Ok(Self::TriangleStrip)
            } else if inner == max_sys::_jit_sym_gl_points {
                Ok(Self::Points)
            } else if inner == max_sys::_jit_sym_gl_lines {
                Ok(Self::Lines)
            } else if inner == max_sys::_jit_sym_gl_line_strip {
                Ok(Self::LineStrip)
            } else if inner == max_sys::_jit_sym_gl_line_loop {
                Ok(Self::LineLoop)
            } else if inner == max_sys::_jit_sym_gl_triangles {
                Ok(Self::Triangles)
            } else if inner == max_sys::_jit_sym_gl_tri_fan {
                Ok(Self::TriangleFan)
            } else if inner == max_sys::_jit_sym_gl_quads {
                Ok(Self::Quads)
            } else if inner == max_sys::_jit_sym_gl_polygon {
                Ok(Self::Polygon)
            } else {
                Err(max_sys::t_jit_error_code::JIT_ERR_GENERIC)
            }
        }
    }
}

impl TryFrom<SymbolRef> for GLGridPrim {
    type Error = max_sys::t_jit_error_code::Type;
    fn try_from(v: SymbolRef) -> Result<Self, Self::Error> {
        unsafe {
            let inner = v.inner();
            if inner == max_sys::_jit_sym_gl_quad_grid {
                Ok(Self::Quad)
            } else if inner == max_sys::_jit_sym_gl_tri_grid {
                Ok(Self::Triangle)
            } else {
                Err(max_sys::t_jit_error_code::JIT_ERR_GENERIC)
            }
        }
    }
}

impl TryFrom<SymbolRef> for GLChunkPrim {
    type Error = max_sys::t_jit_error_code::Type;
    fn try_from(v: SymbolRef) -> Result<Self, Self::Error> {
        if let Ok(p) = GLPrim::try_from(v.clone()) {
            Ok(Self::Prim(p))
        } else {
            GLGridPrim::try_from(v).map(|p| Self::Grid(p))
        }
    }
}
