//! Integration tests for `math::vector::Vector`: arithmetic and its length errors, indexing, and
//! the local linear operations on vectors of secret shares.

use scl_rs::math::{field::secp256k1_prime::Secp256k1PrimeField, ring::Ring, vector::Vector};

/// Checks that the dot product of a random vector with the zero vector is zero.
#[test]
fn dot_with_zero() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len);
    let dot_prod = vector.dot(&zero_vec);
    assert_eq!(dot_prod.unwrap(), Secp256k1PrimeField::ZERO);
}

/// Checks that adding the zero vector leaves a random vector unchanged.
#[test]
fn add_with_zero() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len);
    let add_vec = &vector + &zero_vec;
    assert_eq!(add_vec.unwrap(), vector);
}

/// Checks that `dot` rejects vectors of different lengths: unwrapping its result panics.
#[test]
#[should_panic]
fn incompatible_dot() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    vector.dot(&zero_vec).unwrap();
}

/// Checks that `+` rejects vectors of different lengths: unwrapping its result panics.
#[test]
#[should_panic]
fn incompatible_add_with_zero() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    let add_vec = &vector + &zero_vec;
    add_vec.unwrap();
}

/// Checks that `-` rejects vectors of different lengths: unwrapping its result panics.
#[test]
#[should_panic]
fn incompatible_sub_with_zero() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    let sub = &vector - &zero_vec;
    sub.unwrap();
}

/// Checks that element-wise `*` rejects vectors of different lengths: unwrapping its result panics.
#[test]
#[should_panic]
fn incompatible_mul_with_zero() {
    let mut rng = rand::rng();
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    let mul = &vector * &zero_vec;
    mul.unwrap();
}

use scl_rs::math::field::mersenne61::Mersenne61;

/// Checks `len` and `is_empty` on a length-5 vector and on an empty one.
#[test]
fn len_and_is_empty() {
    let v: Vector<Mersenne61> = Vector::zero(5);
    assert_eq!(v.len(), 5);
    assert!(!v.is_empty());

    let empty: Vector<Mersenne61> = Vector::from(Vec::new());
    assert!(empty.is_empty());
}

/// Checks that indexing reads entries and that a write through `IndexMut` is visible afterwards.
#[test]
fn index_read_and_write() {
    let mut v: Vector<Mersenne61> = Vector::from((1..=3).map(Mersenne61::from).collect::<Vec<_>>());
    assert_eq!(v[0], Mersenne61::from(1));
    assert_eq!(v[2], Mersenne61::from(3));
    v[1] = Mersenne61::from(42);
    assert_eq!(v[1], Mersenne61::from(42));
}

/// Checks the dot product of `[1, 2, 3]` and `[4, 5, 6]` against the hand-computed `32`.
#[test]
fn dot_known_value() {
    let a: Vector<Mersenne61> = Vector::from(
        vec![1, 2, 3]
            .into_iter()
            .map(Mersenne61::from)
            .collect::<Vec<_>>(),
    );
    let b: Vector<Mersenne61> = Vector::from(
        vec![4, 5, 6]
            .into_iter()
            .map(Mersenne61::from)
            .collect::<Vec<_>>(),
    );
    // 1*4 + 2*5 + 3*6 = 32
    assert_eq!(a.dot(&b).unwrap(), Mersenne61::from(32));
}

/// Checks that the all-ones vector of length `n` dotted with itself gives `n`.
#[test]
fn ones_dot_is_length() {
    let n = 10;
    let ones: Vector<Mersenne61> = Vector::ones(n);
    assert_eq!(ones.dot(&ones).unwrap(), Mersenne61::from(n as u64));
}

/// Checks element-wise `*` on `[2, 3]` and `[5, 7]` against the hand-computed `[10, 21]`.
#[test]
fn elementwise_mul_known() {
    let a: Vector<Mersenne61> = Vector::from(
        vec![2, 3]
            .into_iter()
            .map(Mersenne61::from)
            .collect::<Vec<_>>(),
    );
    let b: Vector<Mersenne61> = Vector::from(
        vec![5, 7]
            .into_iter()
            .map(Mersenne61::from)
            .collect::<Vec<_>>(),
    );
    let prod = (&a * &b).unwrap();
    let expected: Vector<Mersenne61> = Vector::from(
        vec![10, 21]
            .into_iter()
            .map(Mersenne61::from)
            .collect::<Vec<_>>(),
    );
    assert_eq!(prod, expected);
}

// --- Vectors of shares -------------------------------------------------------------------------
//
// The operator impls are bounded on the element operations themselves rather than on `Ring`, so a
// `Vector` can also hold shares: adding, subtracting and negating them are the local,
// communication-free operations of a linear scheme (see `LinearShare`). Element-wise `Mul` is
// deliberately *not* reachable for shares — multiplying two shared values is not local.

mod common;

use common::{deal_by_party, open_entry};
use scl_rs::{
    net::PartyId,
    ss::{additive::AdditiveSS, shamir::ShamirSS},
};

/// Checks that each party adding its two vectors of additive shares locally yields shares of the
/// entry-wise sum.
#[test]
fn additive_share_vector_add() {
    const N_PARTIES: usize = 5;
    const LEN: usize = 4;

    let mut rng = rand::rng();
    let parties: Vec<PartyId> = (0..N_PARTIES).map(PartyId::from).collect();
    let a: Vec<Mersenne61> = (0..LEN).map(|_| Mersenne61::random(&mut rng)).collect();
    let b: Vec<Mersenne61> = (0..LEN).map(|_| Mersenne61::random(&mut rng)).collect();

    let va = deal_by_party::<AdditiveSS<Mersenne61>>(&a, &parties, ());
    let vb = deal_by_party::<AdditiveSS<Mersenne61>>(&b, &parties, ());

    // Every party adds its two share vectors locally, with no communication.
    let sums: Vec<Vector<AdditiveSS<Mersenne61>>> = va
        .into_iter()
        .zip(vb.iter())
        .map(|(x, y)| (x + y).unwrap())
        .collect();

    for k in 0..LEN {
        assert_eq!(open_entry(&sums, k, &parties), a[k] + &b[k]);
    }
}

/// Checks that, on vectors of degree-3 Shamir shares held by 7 parties, local subtraction and
/// negation yield shares of `a - b` and `-a` entry-wise.
#[test]
fn shamir_share_vector_sub_and_neg() {
    const N_PARTIES: usize = 7;
    const DEGREE: usize = 3;
    const LEN: usize = 4;

    let mut rng = rand::rng();
    let parties: Vec<PartyId> = (1..=N_PARTIES).map(PartyId::from).collect();
    let a: Vec<Mersenne61> = (0..LEN).map(|_| Mersenne61::random(&mut rng)).collect();
    let b: Vec<Mersenne61> = (0..LEN).map(|_| Mersenne61::random(&mut rng)).collect();

    let va = deal_by_party::<ShamirSS<1, Mersenne61>>(&a, &parties, DEGREE);
    let vb = deal_by_party::<ShamirSS<1, Mersenne61>>(&b, &parties, DEGREE);

    // Subtraction through the reference impl, negation through the owned one.
    let diffs: Vec<Vector<ShamirSS<1, Mersenne61>>> = va
        .iter()
        .zip(vb.iter())
        .map(|(x, y)| (x - y).unwrap())
        .collect();
    let negs: Vec<Vector<ShamirSS<1, Mersenne61>>> = va.into_iter().map(|x| -x).collect();

    for k in 0..LEN {
        assert_eq!(open_entry(&diffs, k, &parties), a[k] - &b[k]);
        assert_eq!(open_entry(&negs, k, &parties), -a[k]);
    }
}

/// Checks that scaling a field vector gives the same result through the owned and borrowed `*`
/// operators, and that every entry is multiplied by the scalar.
#[test]
fn scalar_mul_ring() {
    let mut rng = rand::rng();
    let vector = Vector::<Mersenne61>::random(6, &mut rng);
    let scalar = Mersenne61::random(&mut rng);

    let owned = vector.clone() * &scalar;
    let borrowed = &vector * &scalar;

    assert_eq!(owned, borrowed);
    for k in 0..vector.len() {
        assert_eq!(owned[k], vector[k] * &scalar);
    }
}

/// Checks that `mul_constant` on each party's vector of additive shares yields shares of every
/// entry times the public constant.
#[test]
fn scalar_mul_shares() {
    const N_PARTIES: usize = 5;
    const LEN: usize = 4;

    let mut rng = rand::rng();
    let parties: Vec<PartyId> = (0..N_PARTIES).map(PartyId::from).collect();
    let a: Vec<Mersenne61> = (0..LEN).map(|_| Mersenne61::random(&mut rng)).collect();
    let scalar = Mersenne61::random(&mut rng);

    let va = deal_by_party::<AdditiveSS<Mersenne61>>(&a, &parties, ());

    // Every party scales its share vector by the public constant, with no communication.
    let scaled: Vec<Vector<AdditiveSS<Mersenne61>>> =
        va.into_iter().map(|v| v.mul_constant(&scalar)).collect();

    for (k, secret) in a.iter().enumerate() {
        assert_eq!(open_entry(&scaled, k, &parties), *secret * &scalar);
    }
}
