//! Integration tests for the field `math::field::mersenne61::Mersenne61` of integers modulo
//! `2^61 - 1`: field axioms, inverses, division, and the modulus constant.

use crypto_bigint::U64;
use rand::Rng;
use scl_rs::math::{
    field::{mersenne61::Mersenne61, FieldError, FiniteField},
    ring::Ring,
};
use std::ops::{Add, Mul, Sub};

/// Checks the additive identity and inverse, `a + 0 = a` and `a + (-a) = 0`, for an element built
/// from a random `u64`.
#[test]
fn multiplicative_and_additive_properties() {
    let rnd_value: u64 = rand::rng().next_u64();
    let a = Mersenne61::from(rnd_value);

    let add_identity = a.add(&Mersenne61::ZERO);
    assert!(add_identity.eq(&a));

    let neg_a = a.negate();
    let add_neg = a.add(&neg_a);
    assert!(add_neg.eq(&Mersenne61::ZERO))
}

/// Checks that adding or subtracting zero leaves a random element unchanged.
#[test]
fn zero() {
    let elem = Mersenne61::random(&mut rand::rng());
    let s = elem.add(&Mersenne61::ZERO);
    assert_eq!(elem, s);

    let elem = Mersenne61::random(&mut rand::rng());
    let s = elem.sub(&Mersenne61::ZERO);
    assert_eq!(elem, s);
}

/// Checks that multiplying a random element by one leaves it unchanged.
#[test]
fn one() {
    let elem = Mersenne61::random(&mut rand::rng());
    let s = elem.mul(&Mersenne61::ONE);
    assert_eq!(elem, s);
}

/// Checks that a random element plus its negation is zero.
#[test]
fn negate() {
    let elem = Mersenne61::random(&mut rand::rng());
    let s = elem.add(&elem.negate());
    assert_eq!(s, Mersenne61::ZERO);
}

/// Checks that a random element minus itself is zero.
#[test]
fn subract() {
    let elem = Mersenne61::random(&mut rand::rng());
    let s = elem.sub(&elem);
    assert_eq!(s, Mersenne61::ZERO);
}

/// Checks that a random nonzero element times its inverse is one, over 100 samples.
#[test]
fn inverse() {
    const SAMPLES: usize = 100;
    for _ in 0..SAMPLES {
        let elem = Mersenne61::random_non_zero(&mut rand::rng());
        let s = elem.mul(&elem.inverse().unwrap());
        assert_eq!(s, Mersenne61::ONE);
    }
}

/// Checks the product `2 · 6 = 12`.
#[test]
fn mult_test1() {
    let a = Mersenne61::from(2);
    let b = Mersenne61::from(6);
    let r = Mersenne61::from(12);

    let s = a.mul(&b);
    assert_eq!(s, r);
}

/// Checks that multiplication commutes, over 50 random pairs.
#[test]
fn mult_conmutativity() {
    const SAMPLES: usize = 50;
    for _ in 0..SAMPLES {
        let a = Mersenne61::random(&mut rand::rng());
        let b = Mersenne61::random(&mut rand::rng());
        let mult1 = a.mul(&b);
        let mult2 = b.mul(&a);
        assert_eq!(mult1, mult2);
    }
}

/// Reads `MODULUS`, which forces const-evaluation of the associated constant. Nothing else in the
/// crate reads it, so without this a malformed definition (such as a self-referential one) would
/// compile clean and only fail at the first real use.
#[test]
fn modulus_is_correct() {
    assert_eq!(
        Mersenne61::MODULUS.get(),
        U64::from_be_hex("1FFFFFFFFFFFFFFF")
    );
}

use proptest::prelude::*;

mod common;

/// A `proptest` strategy for uniform `Mersenne61` elements (see `common::field_element`).
fn element() -> impl Strategy<Value = Mersenne61> {
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
        prop_assume!(a != Mersenne61::ZERO);
        prop_assert_eq!(a * &(a.inverse().unwrap()), Mersenne61::ONE)
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
        prop_assert_eq!(a + &Mersenne61::ZERO, a);
    }

    /// Checks that an element plus its negation is zero.
    #[test]
    fn add_inverse_equals_zero(a in element()) {
        prop_assert_eq!(a + &a.negate(), Mersenne61::ZERO);
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
        prop_assert_eq!(a * &Mersenne61::ONE, a);
    }

    /// Checks that an element minus itself is zero.
    #[test]
    fn sub_self_equals_zero(a in element()) {
        prop_assert_eq!(a - &a, Mersenne61::ZERO);
    }

    /// Checks that subtraction is addition of the negation: `a - b = a + (-b)`.
    #[test]
    fn sub_equals_add_negate(a in element(), b in element()) {
        prop_assert_eq!(a - &b, a + &b.negate());
    }

    /// Checks that division undoes multiplication, `(a / b) · b = a`; zero divisors are
    /// discarded.
    #[test]
    fn div_inverts_mul(a in element(), b in element()) {
        prop_assume!(b != Mersenne61::ZERO);
        prop_assert_eq!((a / &b).unwrap() * &b, a);
    }

    /// Checks that dividing by one returns the element unchanged.
    #[test]
    fn div_by_one_is_identity(a in element()) {
        prop_assert_eq!((a / &Mersenne61::ONE).unwrap(), a);
    }

    /// Checks that dividing by zero returns `FieldError::ZeroInverse`.
    #[test]
    fn div_by_zero_errors(a in element()) {
        prop_assert!(matches!(a / &Mersenne61::ZERO, Err(FieldError::ZeroInverse)));
    }
}
