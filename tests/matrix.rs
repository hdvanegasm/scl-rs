//! Integration tests for `math::matrix::Matrix`: construction, indexing, arithmetic and its
//! dimension errors, and the local linear operations on matrices of secret shares.

use scl_rs::{math::field::secp256k1_scalar::Secp256k1ScalarField, prelude::Matrix};

/// Checks that the zero matrix times a random 10 x 10 `Secp256k1ScalarField` matrix is zero.
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

/// Checks that the identity times a random 10 x 10 matrix returns that matrix.
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

/// Builds a `rows x columns` `Mersenne61` matrix from `values` in row-major order; panics if the
/// number of values does not match.
fn matrix(rows: usize, columns: usize, values: &[u64]) -> Matrix<Mersenne61> {
    let elems: Vec<Mersenne61> = values.iter().copied().map(Mersenne61::from).collect();
    Matrix::from_vec(rows, columns, elems).unwrap()
}

/// Checks that `Matrix::from_vec` returns `InvalidDimension` when the element count differs from
/// `rows * columns` and when a dimension is zero.
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

/// Checks the dimensions produced by `identity`, `zero` and `random`, and `is_square` on a square
/// and a non-square matrix.
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

/// Checks that `get` returns `None` when the row or column index is out of range.
#[test]
fn get_out_of_bounds_is_none() {
    let m: Matrix<Mersenne61> = Matrix::zero(2, 3).unwrap();
    assert!(m.get(2, 0).is_none());
    assert!(m.get(0, 3).is_none());
    assert!(m.get(100, 100).is_none());
}

/// Checks that `get(i, j)` reads the elements given to `from_vec` in row-major order.
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

/// Checks that a write through `get_mut` is visible to a later `get`.
#[test]
fn get_mut_writes_through() {
    let mut m: Matrix<Mersenne61> = Matrix::zero(2, 3).unwrap();
    *m.get_mut(1, 2).unwrap() = Mersenne61::from(42);
    assert_eq!(m.get(1, 2), Some(&Mersenne61::from(42)));
}

/// Checks that `scalar_mult` leaves its receiver unchanged despite taking `&mut self`, and that its
/// result equals what `scalar_mut_in_place` writes in place.
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

/// Checks that adding or subtracting a 2 x 3 and a 3 x 2 matrix returns `NotCompatible`.
#[test]
fn add_sub_incompatible_dimensions() {
    let a: Matrix<Mersenne61> = Matrix::zero(2, 3).unwrap();
    let b: Matrix<Mersenne61> = Matrix::zero(3, 2).unwrap();
    assert!(matches!(a.clone() + &b, Err(MatrixError::NotCompatible)));
    assert!(matches!(a - &b, Err(MatrixError::NotCompatible)));
}

/// Checks that multiplying two 2 x 3 matrices, whose inner dimensions differ, returns
/// `NotCompatible`.
#[test]
fn mul_incompatible_dimensions() {
    let a: Matrix<Mersenne61> = Matrix::zero(2, 3).unwrap();
    let b: Matrix<Mersenne61> = Matrix::zero(2, 3).unwrap();
    assert!(matches!(a * &b, Err(MatrixError::NotCompatible)));
}

/// Checks a 2 x 3 by 3 x 2 matrix product against a hand-computed result.
#[test]
fn mul_nonsquare_matrices() {
    let a = matrix(2, 3, &[1, 2, 3, 4, 5, 6]);
    let b = matrix(3, 2, &[7, 8, 9, 10, 11, 12]);
    let expected = matrix(2, 2, &[58, 64, 139, 154]);
    assert_eq!((a * &b).unwrap(), expected);
}

/// Checks a 2 x 3 matrix times a length-3 vector against a hand-computed result.
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

// --- Matrices of shares ------------------------------------------------------------------------
//
// The linear operators are bounded on the element operations rather than on `Ring`, so a `Matrix`
// can hold shares: adding, subtracting and negating them are the local, communication-free
// operations of a linear scheme (see `LinearShare`). The `Mul` family stays out of reach for
// shares by design — multiplying two shared values is not a local operation.

mod common;

use common::deal_by_party;
use scl_rs::{
    math::ring::Ring,
    net::PartyId,
    ss::{additive::AdditiveSS, shamir::ShamirSS, LinearShare},
};

/// Reshapes each party's share vector into a `ROWS x COLUMNS` matrix.
fn as_matrices<S: LinearShare>(
    per_party: Vec<Vector<S>>,
    rows: usize,
    columns: usize,
) -> Vec<Matrix<S>> {
    per_party
        .into_iter()
        .map(|v| Matrix::from_vec(rows, columns, v.into_iter().collect()).unwrap())
        .collect()
}

/// Reconstructs the `(i, j)` entry from a per-party set of share matrices.
fn open_cell<S: LinearShare>(
    per_party: &[Matrix<S>],
    i: usize,
    j: usize,
    parties: &[PartyId],
) -> S::Value {
    let shares: Vec<S> = per_party
        .iter()
        .map(|m| m.get(i, j).unwrap().clone())
        .collect();
    S::secret_from_shares(&shares, parties).unwrap()
}

/// Checks that each party adding its two matrices of additive shares locally yields shares of the
/// entry-wise sum.
#[test]
fn additive_share_matrix_add() {
    const N_PARTIES: usize = 5;
    const ROWS: usize = 2;
    const COLUMNS: usize = 3;

    let mut rng = rand::rng();
    let parties: Vec<PartyId> = (0..N_PARTIES).map(PartyId::from).collect();
    let a: Vec<Mersenne61> = (0..ROWS * COLUMNS)
        .map(|_| Mersenne61::random(&mut rng))
        .collect();
    let b: Vec<Mersenne61> = (0..ROWS * COLUMNS)
        .map(|_| Mersenne61::random(&mut rng))
        .collect();

    let ma = as_matrices(
        deal_by_party::<AdditiveSS<Mersenne61>>(&a, &parties, ()),
        ROWS,
        COLUMNS,
    );
    let mb = as_matrices(
        deal_by_party::<AdditiveSS<Mersenne61>>(&b, &parties, ()),
        ROWS,
        COLUMNS,
    );

    // Every party adds its two share matrices locally, with no communication.
    let sums: Vec<Matrix<AdditiveSS<Mersenne61>>> = ma
        .into_iter()
        .zip(mb.iter())
        .map(|(x, y)| (x + y).unwrap())
        .collect();

    for (k, secret) in a.iter().enumerate() {
        let (i, j) = (k / COLUMNS, k % COLUMNS);
        assert_eq!(open_cell(&sums, i, j, &parties), *secret + &b[k]);
    }
}

/// Checks that, on matrices of degree-3 Shamir shares held by 7 parties, local `-=`, owned `-` and
/// unary negation yield shares of `a - b` and `-a` entry-wise.
#[test]
fn shamir_share_matrix_sub_and_neg() {
    const N_PARTIES: usize = 7;
    const DEGREE: usize = 3;
    const ROWS: usize = 2;
    const COLUMNS: usize = 3;

    let mut rng = rand::rng();
    let parties: Vec<PartyId> = (1..=N_PARTIES).map(PartyId::from).collect();
    let a: Vec<Mersenne61> = (0..ROWS * COLUMNS)
        .map(|_| Mersenne61::random(&mut rng))
        .collect();
    let b: Vec<Mersenne61> = (0..ROWS * COLUMNS)
        .map(|_| Mersenne61::random(&mut rng))
        .collect();

    let ma = as_matrices(
        deal_by_party::<ShamirSS<1, Mersenne61>>(&a, &parties, DEGREE),
        ROWS,
        COLUMNS,
    );
    let mb = as_matrices(
        deal_by_party::<ShamirSS<1, Mersenne61>>(&b, &parties, DEGREE),
        ROWS,
        COLUMNS,
    );

    // `-=` in place, then the owned `-` operator, then unary negation.
    let diffs: Vec<Matrix<ShamirSS<1, Mersenne61>>> = ma
        .iter()
        .zip(mb.iter())
        .map(|(x, y)| {
            let mut acc = x.clone();
            acc -= y;
            acc
        })
        .collect();
    let owned_diffs: Vec<Matrix<ShamirSS<1, Mersenne61>>> = ma
        .iter()
        .zip(mb.iter())
        .map(|(x, y)| (x.clone() - y).unwrap())
        .collect();
    let negs: Vec<Matrix<ShamirSS<1, Mersenne61>>> = ma.into_iter().map(|x| -x).collect();

    for (k, secret) in a.iter().enumerate() {
        let (i, j) = (k / COLUMNS, k % COLUMNS);
        assert_eq!(open_cell(&diffs, i, j, &parties), *secret - &b[k]);
        assert_eq!(open_cell(&owned_diffs, i, j, &parties), *secret - &b[k]);
        assert_eq!(open_cell(&negs, i, j, &parties), -*secret);
    }
}

/// Checks that `mul_constant` on each party's matrix of additive shares yields shares of every
/// entry times the public constant.
#[test]
fn scalar_mul_shares() {
    const N_PARTIES: usize = 5;
    const ROWS: usize = 2;
    const COLUMNS: usize = 3;

    let mut rng = rand::rng();
    let parties: Vec<PartyId> = (0..N_PARTIES).map(PartyId::from).collect();
    let secrets: Vec<Mersenne61> = (0..ROWS * COLUMNS)
        .map(|_| Mersenne61::random(&mut rng))
        .collect();
    let scalar = Mersenne61::random(&mut rng);

    let scaled: Vec<Matrix<AdditiveSS<Mersenne61>>> = as_matrices(
        deal_by_party::<AdditiveSS<Mersenne61>>(&secrets, &parties, ()),
        ROWS,
        COLUMNS,
    )
    .into_iter()
    .map(|m| m.mul_constant(&scalar))
    .collect();

    for (k, secret) in secrets.iter().enumerate() {
        let (i, j) = (k / COLUMNS, k % COLUMNS);
        assert_eq!(open_cell(&scaled, i, j, &parties), *secret * &scalar);
    }
}
