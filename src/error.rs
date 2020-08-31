pub type MaxResult<T> = Result<T, MaxError>;
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
                println!("unknown error code {}", error);
                Err(MaxError::Generic)
            }
        }
    }
}
