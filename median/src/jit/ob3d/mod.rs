//! Callback-scoped GL resources. Keep these inside an ob3d callback; none is Send or Sync.
//! Raw buffer/geometry adoption and geometry upload remain unsafe because the SDK headers
//! do not specify payload ownership or the `setdata` method's retention contract.
//!
//! `GLDataBuffer` and `GLGeometry` are not available on Windows: they call
//! `jit_gl_buffer_data_destroy_tagged` and `jit_ob3d_draw_geometry`, which the pinned SDK
//! declares in `jit.gl.draw.h` but its `jitlib.lib` does not export, so any binary linking
//! them fails to link. They stay unavailable there until a Max build is shown to export them.
#[cfg(not(target_os = "windows"))]
pub mod buffer;
pub mod chunk;
#[cfg(not(target_os = "windows"))]
pub mod geometry;
#[cfg(not(target_os = "windows"))]
pub use buffer::GLDataBuffer;
pub use chunk::{GLChunk, GLChunkPrim, GLGridPrim, GLPrim};
#[cfg(not(target_os = "windows"))]
pub use geometry::GLGeometry;
