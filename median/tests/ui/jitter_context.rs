use median::jit::DrawContext;

fn assert_send<T: Send>() {}

fn main() {
    let _ = DrawContext {};
    assert_send::<DrawContext>();
}
