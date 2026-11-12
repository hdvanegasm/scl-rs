//! Integration tests for `math::field::secp256k1_prime::Secp256k1PrimeField`, the base field over
//! which secp256k1 is defined: field axioms, inverses, division, serialization, and the modulus
//! constant.

use crypto_bigint::U256;
use scl_rs::math::{
    field::{secp256k1_prime::Secp256k1PrimeField, FieldError, FiniteField},
    ring::Ring,
};
use std::ops::{Mul, Sub};

/// Checks that a random element minus itself is zero.
#[test]
fn subtraction_validity() {
    let mut rng = rand::rng();
    let value = Secp256k1PrimeField::random(&mut rng);
    let subtraction = value.sub(&value);
    assert_eq!(subtraction, Secp256k1PrimeField::ZERO)
}

/// Checks that a random nonzero element times its inverse is one.
#[test]
fn inverse() {
    let mut rng = rand::rng();
    let value = Secp256k1PrimeField::random_non_zero(&mut rng);
    let inverse = value.inverse().unwrap();
    let mult = value.mul(&inverse);
    assert_eq!(mult, Secp256k1PrimeField::ONE);
}

/// Reads `MODULUS`, which forces const-evaluation of the associated constant. Nothing else in the
/// crate reads it, so without this a malformed definition (such as a self-referential one) would
/// compile clean and only fail at the first real use.
#[test]
fn modulus_is_correct() {
    assert_eq!(
        Secp256k1PrimeField::MODULUS.get(),
        U256::from_be_hex("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F")
    );
}

use proptest::prelude::*;

use crate::common::roundtrip;

mod common;

/// A `proptest` strategy for uniform `Secp256k1PrimeField` elements (see
/// `common::field_element`).
fn element() -> impl Strategy<Value = Secp256k1PrimeField> {
    common::field_element()
}

proptest! {
    /// Checks that multiplication distributes over addition: `a · (b + c) = a · b + a · c`.
    #[test]
    fn mul_distributes_over_add(a in element(), b in element(), c in element()) {
        prop_assert_eq!(a * &(b + &c), (a * &b) + &(a * &c));
    }

    /// Checks that a nonzero element times its inverse is one; zero inputs are discarded.
    #[test]
    fn mul_inverse_equals_one(a in element()) {
        prop_assume!(a != Secp256k1PrimeField::ZERO);
        prop_assert_eq!(a * &(a.inverse().unwrap()), Secp256k1PrimeField::ONE)
    }

    /// Checks that addition commutes: `a + b = b + a`.
    #[test]
    fn add_commutes(a in element(), b in element()) {
        prop_assert_eq!(a + &b, b + &a);
    }

    /// Checks that addition associates: `(a + b) + c = a + (b + c)`.
    #[test]
    fn add_associates(a in element(), b in element(), c in element()) {
        prop_assert_eq!((a + &b) + &c, a + &(b + &c));
    }

    /// Checks that zero is the additive identity: `a + 0 = a`.
    #[test]
    fn add_identity(a in element()) {
        prop_assert_eq!(a + &Secp256k1PrimeField::ZERO, a);
    }

    /// Checks that an element plus its negation is zero.
    #[test]
    fn add_inverse_equals_zero(a in element()) {
        prop_assert_eq!(a + &a.negate(), Secp256k1PrimeField::ZERO);
    }

    /// Checks that multiplication commutes: `a · b = b · a`.
    #[test]
    fn mul_commutes(a in element(), b in element()) {
        prop_assert_eq!(a * &b, b * &a);
    }

    /// Checks that multiplication associates: `(a · b) · c = a · (b · c)`.
    #[test]
    fn mul_associates(a in element(), b in element(), c in element()) {
        prop_assert_eq!((a * &b) * &c, a * &(b * &c));
    }

    /// Checks that one is the multiplicative identity: `a · 1 = a`.
    #[test]
    fn mul_identity(a in element()) {
        prop_assert_eq!(a * &Secp256k1PrimeField::ONE, a);
    }

    /// Checks that an element minus itself is zero.
    #[test]
    fn sub_self_equals_zero(a in element()) {
        prop_assert_eq!(a - &a, Secp256k1PrimeField::ZERO);
    }

    /// Checks that subtraction is addition of the negation: `a - b = a + (-b)`.
    #[test]
    fn sub_equals_add_negate(a in element(), b in element()) {
        prop_assert_eq!(a - &b, a + &b.negate());
    }

    /// Checks that an element survives a `postcard` serialization round-trip unchanged.
    #[test]
    fn postcard_roundtrip(a in element()) {
        roundtrip(a)?;
    }

    /// Checks that division undoes multiplication, `(a / b) · b = a`; zero divisors are
    /// discarded.
    #[test]
    fn div_inverts_mul(a in element(), b in element()) {
        prop_assume!(b != Secp256k1PrimeField::ZERO);
        prop_assert_eq!((a / &b).unwrap() * &b, a);
    }

    /// Checks that dividing by one returns the element unchanged.
    #[test]
    fn div_by_one_is_identity(a in element()) {
        prop_assert_eq!((a / &Secp256k1PrimeField::ONE).unwrap(), a);
    }

    /// Checks that dividing by zero returns `FieldError::ZeroInverse`.
    #[test]
    fn div_by_zero_errors(a in element()) {
        prop_assert!(matches!(a / &Secp256k1PrimeField::ZERO, Err(FieldError::ZeroInverse)));
    }
}
