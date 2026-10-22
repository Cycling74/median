//! Wrapping the max error codes into a rust enum.

pub type MaxResult<T> = Result<T, MaxError>;

#[derive(Copy, Clone, Debug)]
pub enum MaxError {
    Generic,
    InvalidPtr,
    Duplicate,
    OutOfMem,
}

impl MaxError {
    pub fn from<T>(error: max_sys::e_max_errorcodes::Type, v: T) -> MaxResult<T> {
        match error {
            max_sys::e_max_errorcodes::MAX_ERR_NONE => Ok(v),
            max_sys::e_max_errorcodes::MAX_ERR_GENERIC => Err(MaxError::Generic),
            max_sys::e_max_errorcodes::MAX_ERR_INVALID_PTR => Err(MaxError::InvalidPtr),
            max_sys::e_max_errorcodes::MAX_ERR_DUPLICATE => Err(MaxError::Duplicate),
            max_sys::e_max_errorcodes::MAX_ERR_OUT_OF_MEM => Err(MaxError::OutOfMem),
            _ => {
                crate::max::error(format!("unknown error code {}", error));
                Err(MaxError::Generic)
            }
        }
    }
}

/// Why your object's `new` refused to make the object: the message is posted to the Max console
/// as an error for the object, and Max shows the box as broken (as it does for a C external
/// whose `new` returns null).
///
/// Made from a message: `String`, `&str`, or any `Display` with [`NewError::new`]. `?` works
/// directly on a `Result` whose error is a `String`, a `&str`, a boxed error, one of the
/// common standard library errors (parse errors, `Utf8Error`, `NulError`, ...), a
/// [`MaxError`] or an outlet's [`StackOverflow`](crate::outlet::StackOverflow). For any other
/// error type use `.map_err(NewError::new)?`. (A blanket `From<E: Error>` is not possible next to
/// `From<String>`: Rust reserves the right for `String` to implement `Error` one day.)
///
/// ```
/// # use median::error::NewError;
/// fn parse(arg: &str) -> Result<i64, NewError> {
///     let n: i64 = arg.parse()?; // ParseIntError
///     if n < 0 {
///         return Err(format!("{n} is negative").into());
///     }
///     Ok(n)
/// }
/// assert_eq!(parse("-1").unwrap_err().to_string(), "-1 is negative");
/// assert_eq!(parse("x").unwrap_err().to_string(), "invalid digit found in string");
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct NewError(String);

impl NewError {
    /// An error with the text of `msg`.
    pub fn new(msg: impl std::fmt::Display) -> Self {
        Self(msg.to_string())
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Debug for NewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NewError({:?})", self.0)
    }
}

impl From<String> for NewError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for NewError {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<MaxError> for NewError {
    fn from(e: MaxError) -> Self {
        Self(format!("Max error: {:?}", e))
    }
}

macro_rules! new_error_from {
    ($($t:ty),* $(,)?) => {
        $(
            impl From<$t> for NewError {
                fn from(e: $t) -> Self {
                    Self(e.to_string())
                }
            }
        )*
    };
}

new_error_from!(
    Box<dyn std::error::Error>,
    Box<dyn std::error::Error + Send + Sync>,
    std::num::ParseIntError,
    std::num::ParseFloatError,
    std::num::TryFromIntError,
    std::str::Utf8Error,
    std::string::FromUtf8Error,
    std::ffi::NulError,
    std::fmt::Error,
    std::io::Error,
    crate::outlet::StackOverflow,
);

#[cfg(test)]
mod tests {
    use super::*;

    fn int(s: &str) -> Result<i32, NewError> {
        Ok(s.parse::<i32>()?)
    }

    #[test]
    fn new_error_conversions() {
        assert_eq!(NewError::from("a").to_string(), "a");
        assert_eq!(NewError::from(String::from("b")).message(), "b");
        assert_eq!(NewError::new(3.5).to_string(), "3.5");
        assert_eq!(int("12"), Ok(12));
        assert_eq!(
            int("x").unwrap_err().to_string(),
            "invalid digit found in string"
        );
        let boxed: Box<dyn std::error::Error + Send + Sync> = "boxed".into();
        assert_eq!(NewError::from(boxed).to_string(), "boxed");
        assert_eq!(
            NewError::from(crate::outlet::StackOverflow).to_string(),
            "stack overflow"
        );
        assert_eq!(format!("{:?}", NewError::from("q")), "NewError(\"q\")");
    }
}
