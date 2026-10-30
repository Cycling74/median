use median::jit::{
    DrawContext,
    ob3d::{GLChunk, GLPrim},
};
use median_mock as _;

// ACCEPT
fn escape(context: &DrawContext) -> GLChunk<'_> {
    // REJECT: fn escape(context: &DrawContext) -> GLChunk<'static> {
    GLChunk::new(context, GLPrim::Triangles, 3, 3, None).unwrap()
}

fn main() {}
