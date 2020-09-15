pub mod class;
pub mod error;
pub mod object;
pub mod symbol;
pub mod types;
pub mod wrapper;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
