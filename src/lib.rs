pub mod class;
//pub mod clock;
pub mod error;
pub mod num;
pub mod object;
pub mod symbol;
pub mod wrapper;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
