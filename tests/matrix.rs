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

use scl_rs::math::{field::mersenne61::Mersenne61, matrix::Error as MatrixError, vector::Vector};

fn matrix(rows: usize, columns: usize, values: &[u64]) -> Matrix<Mersenne61> {
    let elems: Vec<Mersenne61> = values.iter().copied().map(Mersenne61::from).collect();
    Matrix::from_vec(rows, columns, elems).unwrap()
}

#[test]
fn from_vec_dimension_mismatch() {
    // 2 * 3 = 6 elements expected, only 5 provided.
    let elems: Vec<Mersenne61> = (1..=5).map(Mersenne61::from).collect();
    assert!(matches!(
        Matrix::from_vec(2, 3, elems),
        Err(MatrixError::InvalidDimension(2, 3))
    ));

    // A zero dimension is rejected.
    assert!(matches!(
        Matrix::from_vec(0, 3, Vec::<Mersenne61>::new()),
        Err(MatrixError::InvalidDimension(0, 3))
    ));
}

#[test]
fn constructor_shapes() {
    let id: Matrix<Mersenne61> = Matrix::identity(4).unwrap();
    assert_eq!((id.rows, id.columns), (4, 4));
    assert!(id.is_square());

    let z: Matrix<Mersenne61> = Matrix::zero(2, 5).unwrap();
    assert_eq!((z.rows, z.columns), (2, 5));
    assert!(!z.is_square());

    let r: Matrix<Mersenne61> = Matrix::random(3, 7, &mut rand::rng()).unwrap();
    assert_eq!((r.rows, r.columns), (3, 7));
}

#[test]
fn get_out_of_bounds_is_none() {
    let m: Matrix<Mersenne61> = Matrix::zero(2, 3).unwrap();
    assert!(m.get(2, 0).is_none());
    assert!(m.get(0, 3).is_none());
    assert!(m.get(100, 100).is_none());
}

#[test]
fn get_returns_row_major_element() {
    // Row-major layout: row 0 = [1, 2, 3], row 1 = [4, 5, 6].
    let elems: Vec<Mersenne61> = (1..=6).map(Mersenne61::from).collect();
    let m = Matrix::from_vec(2, 3, elems).unwrap();
    assert_eq!(m.get(0, 0), Some(&Mersenne61::from(1)));
    assert_eq!(m.get(0, 2), Some(&Mersenne61::from(3)));
    assert_eq!(m.get(1, 0), Some(&Mersenne61::from(4)));
    assert_eq!(m.get(1, 2), Some(&Mersenne61::from(6)));
}

#[test]
fn get_mut_writes_through() {
    let mut m: Matrix<Mersenne61> = Matrix::zero(2, 3).unwrap();
    *m.get_mut(1, 2).unwrap() = Mersenne61::from(42);
    assert_eq!(m.get(1, 2), Some(&Mersenne61::from(42)));
}

#[test]
fn scalar_mult_does_not_mutate_but_in_place_does() {
    let mut rng = rand::rng();
    let original: Matrix<Mersenne61> = Matrix::random(4, 4, &mut rng).unwrap();
    let k = Mersenne61::from(7);

    // `scalar_mult` takes `&mut self` but returns a fresh matrix WITHOUT mutating the
    // receiver (a misleading signature — see the design note).
    let mut a = original.clone();
    let scaled = a.scalar_mult(&k);
    assert_eq!(a, original, "scalar_mult unexpectedly mutated the receiver");

    // `scalar_mut_in_place` mutates in place.
    let mut b = original.clone();
    b.scalar_mut_in_place(&k);

    assert_eq!(scaled, b);
}

#[test]
fn add_sub_incompatible_dimensions() {
    let a: Matrix<Mersenne61> = Matrix::zero(2, 3).unwrap();
    let b: Matrix<Mersenne61> = Matrix::zero(3, 2).unwrap();
    assert!(matches!(a.clone() + &b, Err(MatrixError::NotCompatible)));
    assert!(matches!(a - &b, Err(MatrixError::NotCompatible)));
}

#[test]
fn mul_incompatible_dimensions() {
    let a: Matrix<Mersenne61> = Matrix::zero(2, 3).unwrap();
    let b: Matrix<Mersenne61> = Matrix::zero(2, 3).unwrap();
    assert!(matches!(a * &b, Err(MatrixError::NotCompatible)));
}

#[test]
fn mul_nonsquare_matrices() {
    let a = matrix(2, 3, &[1, 2, 3, 4, 5, 6]);
    let b = matrix(3, 2, &[7, 8, 9, 10, 11, 12]);
    let expected = matrix(2, 2, &[58, 64, 139, 154]);
    assert_eq!((a * &b).unwrap(), expected);
}

#[test]
fn mul_nonsquare_matrix_by_vector() {
    let a = matrix(2, 3, &[1, 2, 3, 4, 5, 6]);
    let v: Vector<Mersenne61> = Vector::from(
        vec![1, 2, 3]
            .into_iter()
            .map(Mersenne61::from)
            .collect::<Vec<_>>(),
    );

    let expected: Vector<Mersenne61> = Vector::from(
        vec![14, 32]
            .into_iter()
            .map(Mersenne61::from)
            .collect::<Vec<_>>(),
    );
    assert_eq!((&a * &v).unwrap(), expected);
}
