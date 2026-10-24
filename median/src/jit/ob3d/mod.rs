//! Callback-scoped GL resources. Keep these inside an ob3d callback; none is Send or Sync.
//! Raw buffer/geometry adoption and geometry upload remain unsafe because the SDK headers
//! do not specify payload ownership or the `setdata` method's retention contract.
pub mod buffer;
pub mod chunk;
pub mod geometry;
pub use buffer::GLDataBuffer;
pub use chunk::{GLChunk, GLChunkPrim, GLGridPrim, GLPrim};
pub use geometry::GLGeometry;
