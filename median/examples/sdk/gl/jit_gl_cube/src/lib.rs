//! A port of the SDK's `gl/jit.gl.cube`: an ob3d that draws a textured cube as one GL chunk of
//! twelve triangles.
//!
//! The C builds the chunk once in `new` and keeps it; median's `GLChunk` lives only inside a GL
//! callback, so `draw` builds it from the same table on every call.

use median::{
    jit::{
        DrawContext, JitBuilder, JitResult, Ob3d,
        ob3d::{GLChunk, GLPrim},
    },
    max_sys::{t_jit_error_code as err, t_jit_gl_chunk_flags as flags},
    prelude::*,
};

const PLANES: usize = 13;
const VERTICES: usize = 12 * 3;

// Texture coordinates of the cube's unfolded cross.
const X1: f32 = 0.0;
const X2: f32 = 0.25;
const X3: f32 = 0.5;
const X4: f32 = 0.75;
const X5: f32 = 1.0;
const Y1: f32 = 0.125;
const Y2: f32 = 0.375;
const Y3: f32 = 0.625;
const Y4: f32 = 0.875;

/// One vertex: position, texture coordinate, then the eight planes up to the edge flag (plane
/// 12), which is set on the first two vertices of each triangle.
const fn v(x: f32, y: f32, z: f32, s: f32, t: f32, edge: bool) -> [f32; PLANES] {
    let e = if edge { 1.0 } else { 0.0 };
    [x, y, z, s, t, 0., 0., 0., 0., 0., 0., 0., e]
}

#[rustfmt::skip]
const CUBE: [[f32; PLANES]; VERTICES] = [
    v(-1., 1., 1., X2, Y3, true), v(-1., 1., -1., X2, Y4, true), v(1., 1., -1., X3, Y4, false),
    v(1., 1., -1., X3, Y4, true), v(1., 1., 1., X3, Y3, true), v(-1., 1., 1., X2, Y3, false),
    v(-1., -1., -1., X1, Y2, true), v(-1., 1., -1., X1, Y3, true), v(-1., 1., 1., X2, Y3, false),
    v(-1., 1., 1., X2, Y3, true), v(-1., -1., 1., X2, Y2, true), v(-1., -1., -1., X1, Y2, false),
    v(-1., -1., 1., X2, Y2, true), v(-1., 1., 1., X2, Y3, true), v(1., 1., 1., X3, Y3, false),
    v(1., 1., 1., X3, Y3, true), v(1., -1., 1., X3, Y2, true), v(-1., -1., 1., X2, Y2, false),
    v(1., -1., 1., X3, Y2, true), v(1., 1., 1., X3, Y3, true), v(1., 1., -1., X4, Y3, false),
    v(1., 1., -1., X4, Y3, true), v(1., -1., -1., X4, Y2, true), v(1., -1., 1., X3, Y2, false),
    v(1., -1., -1., X4, Y2, true), v(1., 1., -1., X4, Y3, true), v(-1., 1., -1., X5, Y3, false),
    v(-1., 1., -1., X5, Y3, true), v(-1., -1., -1., X5, Y2, true), v(1., -1., -1., X4, Y2, false),
    v(-1., -1., -1., X2, Y1, true), v(-1., -1., 1., X2, Y2, true), v(1., -1., 1., X3, Y2, false),
    v(1., -1., 1., X3, Y2, true), v(1., -1., -1., X3, Y1, true), v(-1., -1., -1., X2, Y1, false),
];

#[derive(JitGlObj)]
#[max(name = "jit.gl.median.cube", no_methods)]
pub struct Cube;

impl Ob3d for Cube {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self)
    }

    fn draw(&self, context: &DrawContext) -> JitResult<()> {
        let mut chunk = GLChunk::new(context, GLPrim::Triangles, PLANES, VERTICES, None)?;
        // SAFETY: the chunk was made above and nothing else holds it. jit.gl.render generates
        // the normals, and the color planes are unused.
        unsafe {
            (*chunk.as_ptr()).m_flags =
                (flags::JIT_GL_CHUNK_IGNORE_COLORS | flags::JIT_GL_CHUNK_IGNORE_NORMALS) as _;
        }
        {
            let mut vertices = chunk.vertices()?;
            let mut lock = vertices.lock();
            let mut data = lock.data()?;
            let mut source = CUBE.iter();
            // SAFETY: the chunk's matrix was made in this call and no one else has seen it.
            for mut rows in unsafe { data.chunks_mut::<f32>()? } {
                for cell in rows.entry_iter_mut() {
                    cell.copy_from_slice(source.next().ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?);
                }
            }
        }
        // SAFETY: every vertex was written above, and the chunk is ours until it drops.
        unsafe { chunk.draw() }
    }
}

median::ext_main! { Cube }

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_void;

    #[test]
    fn every_vertex_is_a_corner_and_every_triangle_flags_two_edges() {
        for (i, vertex) in CUBE.iter().enumerate() {
            assert!(vertex[..3].iter().all(|c| c.abs() == 1.0), "vertex {i}");
            assert_eq!(vertex[12], if i % 3 == 2 { 0.0 } else { 1.0 }, "vertex {i}");
        }
        // Each face is one axis held at +-1 by both of its triangles.
        for face in CUBE.chunks(6) {
            let fixed = (0..3).filter(|&a| face.iter().all(|v| v[a] == face[0][a]));
            assert_eq!(fixed.count(), 1);
        }
    }

    #[test]
    fn draw_makes_draws_and_releases_a_chunk() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.gl.median.cube", &["ctx".into()]);
        assert!(!x.is_null(), "{:?}", k.console_log());
        assert_eq!(k.outlet_count(x), 2);
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "ob3d_draw").unwrap();
        // SAFETY: `ob3d_draw` is registered with this signature
        let draw: unsafe extern "C" fn(*mut c_void) -> median::max_sys::t_jit_err =
            unsafe { std::mem::transmute(f) };
        // SAFETY: a live core
        assert_eq!(unsafe { draw(core) }, 0);
        assert_eq!(median_mock::jit::live_chunk_count(), 0);
        assert!(k.take_console_log().is_empty());
        assert!(k.free_object(x));
        assert_eq!(median_mock::jit::context_count(), 0);
    }
}
