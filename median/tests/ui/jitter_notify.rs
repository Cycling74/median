use median::{
    jit::{
        JitBuilder, JitObjRef, JitResult, MatrixOp,
        matrix::{Count, IOCount, Matrix},
    },
    notify::Notification,
    prelude::*,
};

// a `#[notify]` method on a Jitter type: the SDK wrapper owns `notify`
#[derive(JitMatrixObj)]
#[max(name = "jit.bad.notify")]
struct Operator {
    _obj: JitObjRef,
}

impl MatrixOp for Operator {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self { _obj: b.obj() })
    }
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }
    fn calc(&self, _: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        Ok(())
    }
}

#[median::methods]
impl Operator {
    #[notify]
    fn notified(&self, _n: &Notification) {}
}

median::ext_main! { Operator }

fn main() {}
