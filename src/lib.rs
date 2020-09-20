pub mod alloc;
pub mod class;
pub mod clock;
pub mod error;
pub mod num;
pub mod object;
pub mod symbol;
pub mod wrapper;

//re-exports
mod max;
pub use self::max::*;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
