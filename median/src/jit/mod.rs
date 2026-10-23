//! Jitter matrix utilities. Matrix memory is externally owned: creating Rust references to it
//! requires an explicit access contract; a Jitter `lock` alone is not a Rust exclusive borrow.

pub mod matrix;
pub mod wrapper;
pub use wrapper::{DrawContext, JitBuilder, JitObjRef, MatrixOp, Ob3d};

pub type JitError = max_sys::t_jit_error_code::Type;
pub type JitResult<T> = Result<T, JitError>;
