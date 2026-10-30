use median::jit::DrawContext;
use median_mock as _;
fn assert_send<T: Send>() {}
fn main() {
    // ACCEPT
    assert_send::<()>();
    // REJECT: assert_send::<DrawContext>();
}
