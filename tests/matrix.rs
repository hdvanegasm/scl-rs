use scl_rs::{math::field::secp256k1_scalar::Secp256k1ScalarField, prelude::Matrix};

#[test]
fn multiplication_by_zero_equals_zero() {
    const ROWS: usize = 10;
    const COLS: usize = 10;
    let mut rng = rand::rng();
    let zero_mat: Matrix<Secp256k1ScalarField> = Matrix::zero(ROWS, COLS).unwrap();
    let rand_mat = Matrix::random(ROWS, COLS, &mut rng).unwrap();
    let mult = (zero_mat * &rand_mat).unwrap();
    assert_eq!(mult, Matrix::zero(ROWS, COLS).unwrap());
}

#[test]
fn multiplication_by_identity() {
    const ROWS: usize = 10;
    const COLS: usize = 10;
    let mut rng = rand::rng();
    let id_mat: Matrix<Secp256k1ScalarField> = Matrix::identity(ROWS).unwrap();
    let rand_mat = Matrix::random(ROWS, COLS, &mut rng).unwrap();
    let mult = (id_mat * &rand_mat).unwrap();
    assert_eq!(mult, rand_mat);
}
