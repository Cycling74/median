use median::jit::{DrawContext, ob3d::{GLChunk, GLPrim}};

fn escape(context: &DrawContext) -> GLChunk<'static> {
    GLChunk::new(context, GLPrim::Triangles, 3, 3, None).unwrap()
}

fn main() {}
