use median::jit::matrix::{MatrixDataType, MatrixInfo, iter::*};

fn main() {
    let info = MatrixInfo::new(MatrixDataType::Int32, 1, &[2]).unwrap();
    let mut data = [1i32, 2];
    let mut chunks = Matrix2DChunkIterMut::new(&mut data, &info).unwrap();
    let mut chunk = chunks.next().unwrap();
    let mut first = chunk.entry_iter_mut();
    let mut second = chunk.entry_iter_mut();
    first.next().unwrap()[0] = 3;
    second.next().unwrap()[0] = 4;
}
