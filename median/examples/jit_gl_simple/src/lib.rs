//! The SDK's simple ob3d quad. Jitter manages its destination, GL context, iolets and assist.
use median::{
    jit::{JitBuilder, JitResult, Ob3d},
    prelude::*,
};

#[derive(MaxObject)]
#[max(name = "jit.gl.median.simple", kind = gl, no_methods)]
pub struct Simple;
impl Ob3d for Simple {
    fn new(_: &mut JitBuilder) -> Result<Self, NewError> {
        Ok(Self)
    }
    fn flags() -> median::max_sys::t_jit_ob3d_flags::Type {
        median::max_sys::t_jit_ob3d_flags::JIT_OB3D_NO_MATRIXOUTPUT
    }
    fn draw(&self, _context: &median::jit::DrawContext) -> JitResult<()> {
        // SAFETY: ob3d calls draw only with its render context and GL state prepared. Do not
        // send ob3d_draw directly from a Max message; it is registered A_CANT for that reason.
        unsafe {
            median::max_sys::jit_gl_immediate_begin(median::max_sys::e_jit_state::JIT_STATE_QUADS);
            median::max_sys::jit_gl_immediate_vertex3f(-1., -1., 0.);
            median::max_sys::jit_gl_immediate_vertex3f(-1., 1., 0.);
            median::max_sys::jit_gl_immediate_vertex3f(1., 1., 0.);
            median::max_sys::jit_gl_immediate_vertex3f(1., -1., 0.);
            median::max_sys::jit_gl_immediate_end();
        }
        Ok(())
    }
}
median::ext_main! { Simple }
