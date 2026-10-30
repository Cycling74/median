use median::jit::DrawContext;
use median_mock as _;
fn main() {
    // ACCEPT
    let _ = std::mem::size_of::<DrawContext>();
    // REJECT: let _ = DrawContext {};
}
