//! Integration tests for the secp256k1 curve group (`math::ec::secp256k1::Secp256k1`): point
//! arithmetic, the point at infinity, on-curve validation, and serialization.

use scl_rs::math::{
    ec::{secp256k1::Secp256k1, EllipticCurve},
    field::{secp256k1_prime::Secp256k1PrimeField, secp256k1_scalar::Secp256k1ScalarField},
    ring::Ring,
};

/// Checks that `P - P` is the point at infinity for `P = k · G` with random nonzero `k`.
#[test]
fn subtraction_validity() {
    let mut rng = rand::rng();
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);
    let sub = curv_elem.sub(&curv_elem);
    assert!(sub.is_point_at_infinity())
}

/// Checks that `P + P` equals `P.dbl()` for `P = k · G` with random nonzero `k`.
#[test]
fn add_and_dbl_compatibility() {
    let mut rng = rand::rng();
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);

    // AddPoint = CurvElem + CurvElem = 2 * CurvElem
    let add_point = curv_elem.add(&curv_elem);
    assert!(add_point.eq(&curv_elem.dbl()))
}

/// Checks that `P + P` equals the scalar multiple `2 · P` for `P = k · G` with random nonzero `k`.
#[test]
fn add_and_scalar_mul_compatibility() {
    let mut rng = rand::rng();
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);

    // AddPoint = CurvElem + CurvElem = 2 * CurvElem
    let add_point = curv_elem.add(&curv_elem);

    let scalar_mul =
        curv_elem.scalar_mul(&(Secp256k1ScalarField::ONE + &Secp256k1ScalarField::ONE));
    println!("{:?}", add_point);
    println!("{:?}", scalar_mul);

    assert!(add_point.eq(&scalar_mul))
}

/// Checks that the generator's affine coordinates satisfy `y^2 = x^3 + 7`.
#[test]
fn generator_is_valid() {
    assert!(Secp256k1::gen().to_affine().is_valid());
}

/// Checks that the scalar multiple `2 · P` equals `P.dbl()` for `P = k · G` with random
/// nonzero `k`.
#[test]
fn dbl_and_scalar_mul_compatibility() {
    let mut rng = rand::rng();
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);

    let add_point = curv_elem.scalar_mul(&(Secp256k1ScalarField::ONE + &Secp256k1ScalarField::ONE));
    assert!(add_point.eq(&curv_elem.dbl()))
}

/// Checks that `1 · P = P` for `P = k · G` with random nonzero `k`.
#[test]
fn identity_scalar_mul() {
    let mut rng = rand::rng();
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);
    let mult = curv_elem.scalar_mul(&Secp256k1ScalarField::ONE);
    assert!(mult.eq(&curv_elem));
}

/// Checks that `0 · P` is the point at infinity for `P = k · G` with random nonzero `k`, after
/// checking that `P` itself is not.
#[test]
fn zero_scalar_mul() {
    let mut rng = rand::rng();
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);
    assert!(!curv_elem.is_point_at_infinity());

    let mult = curv_elem.scalar_mul(&Secp256k1ScalarField::ZERO);
    assert!(mult.is_point_at_infinity());
}

/// Checks that doubling the point at infinity gives the point at infinity.
#[test]
fn dbl_of_point_at_infinity() {
    assert!(Secp256k1::POINT_AT_INFINITY.dbl().is_point_at_infinity())
}

/// Checks that multiplying the point at infinity by a random nonzero scalar gives the point at
/// infinity.
#[test]
fn scalar_mul_point_at_infinity() {
    let mut rng = rand::rng();
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    assert!(Secp256k1::POINT_AT_INFINITY
        .scalar_mul(&coeff)
        .is_point_at_infinity())
}

/// Checks that `is_point_at_infinity` holds for `Secp256k1::POINT_AT_INFINITY`.
#[test]
fn point_at_infinity_is_checked_correctly() {
    assert!(Secp256k1::POINT_AT_INFINITY.is_point_at_infinity())
}

/// Checks that the point at infinity plus itself is the point at infinity.
#[test]
fn addition_between_two_zeros() {
    assert!(Secp256k1::POINT_AT_INFINITY
        .add(&Secp256k1::POINT_AT_INFINITY)
        .is_point_at_infinity())
}

/// Checks that adding the point at infinity to `P` returns `P`, for `P = k · G` with random
/// nonzero `k`.
#[test]
fn elliptic_curve_identity() {
    let mut rng = rand::rng();
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);

    assert!(curv_elem.add(&Secp256k1::POINT_AT_INFINITY).eq(&curv_elem));
}

/// Checks that `Secp256k1::try_from`, the on-curve check that deserialization runs, accepts the
/// generator's projective coordinates.
#[test]
fn on_curve_point_passes_validation() {
    // The generator is on the curve, so the deserialization check must accept it.
    let g = Secp256k1::gen();
    let coords = (*g.x(), *g.y(), *g.z());
    assert!(Secp256k1::try_from(coords).is_ok());
}

/// Checks that `Secp256k1::try_from` rejects the generator's coordinates with `y` replaced by
/// `123`, which no longer satisfy the curve equation.
#[test]
fn off_curve_point_is_rejected() {
    // Tamper with the y-coordinate of the generator so it no longer satisfies
    // y^2 = x^3 + 7; the deserialization check must reject it.
    let g = Secp256k1::gen();
    let bad = (*g.x(), Secp256k1PrimeField::from(123u64), *g.z());
    assert!(Secp256k1::try_from(bad).is_err());
}

use proptest::prelude::*;

use crate::common::roundtrip;
mod common;

/// A `proptest` strategy for points `k · G` with `k` a uniform scalar (see
/// `common::curve_element`).
fn element() -> impl Strategy<Value = Secp256k1> {
    common::curve_element()
}

proptest! {
    /// Checks that a point `k · G` survives a `postcard` serialization round-trip unchanged.
    #[test]
    fn postcard_roundtrip(g in element()) {
        roundtrip(g)?;
    }
}
