use scl_rs::math::{
    field::{secp256k1_scalar::Secp256k1ScalarField, FiniteField},
    ring::Ring,
};
use std::ops::{Add, Mul, Sub};

#[test]
fn subtraction_validity() {
    let mut rng = rand::rng();
    let value = Secp256k1ScalarField::random(&mut rng);
    let subtraction = value.sub(&value);
    assert_eq!(subtraction, Secp256k1ScalarField::ZERO)
}

#[test]
fn inverse_correctness() {
    let mut rng = rand::rng();
    let value = Secp256k1ScalarField::random_non_zero(&mut rng);
    let inverse = value.inverse().unwrap();
    let mult = value.mul(&inverse);
    assert_eq!(mult, Secp256k1ScalarField::ONE);
}

#[test]
fn addition_of_one_gives_two() {
    let sum = Secp256k1ScalarField::ONE.add(&Secp256k1ScalarField::ONE);
    let two = Secp256k1ScalarField::from(2);
    assert_eq!(sum, two);
}

use proptest::prelude::*;

use crate::common::roundtrip;

mod common;

fn element() -> impl Strategy<Value = Secp256k1ScalarField> {
    common::field_element()
}

proptest! {
    #[test]
    fn mul_distributes_over_add(a in element(), b in element(), c in element()) {
        prop_assert_eq!(a * &(b + &c), (a * &b) + &(a * &c));
    }

    #[test]
    fn mul_inverse_equals_one(a in element()) {
        prop_assume!(a != Secp256k1ScalarField::ZERO);
        prop_assert_eq!(a * &(a.inverse().unwrap()), Secp256k1ScalarField::ONE)
    }

    #[test]
    fn add_commutes(a in element(), b in element()) {
        prop_assert_eq!(a + &b, b + &a);
    }

    #[test]
    fn add_associates(a in element(), b in element(), c in element()) {
        prop_assert_eq!((a + &b) + &c, a + &(b + &c));
    }

    #[test]
    fn add_identity(a in element()) {
        prop_assert_eq!(a + &Secp256k1ScalarField::ZERO, a);
    }

    #[test]
    fn add_inverse_equals_zero(a in element()) {
        prop_assert_eq!(a + &a.negate(), Secp256k1ScalarField::ZERO);
    }

    #[test]
    fn mul_commutes(a in element(), b in element()) {
        prop_assert_eq!(a * &b, b * &a);
    }

    #[test]
    fn mul_associates(a in element(), b in element(), c in element()) {
        prop_assert_eq!((a * &b) * &c, a * &(b * &c));
    }

    #[test]
    fn mul_identity(a in element()) {
        prop_assert_eq!(a * &Secp256k1ScalarField::ONE, a);
    }

    #[test]
    fn sub_self_equals_zero(a in element()) {
        prop_assert_eq!(a - &a, Secp256k1ScalarField::ZERO);
    }

    #[test]
    fn sub_equals_add_negate(a in element(), b in element()) {
        prop_assert_eq!(a - &b, a + &b.negate());
    }

    #[test]
    fn postcard_roundtrip(a in element()) {
        roundtrip(a)?;
    }
}
