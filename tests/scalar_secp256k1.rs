use crypto_bigint::rand_core::OsRng;
use scl_rs::math::{
    field::{secp256k1_scalar::Secp256k1ScalarField, FiniteField},
    ring::Ring,
};
use std::ops::{Add, Div, Mul, Sub};

#[test]
fn subtraction_validity() {
    let mut rng = OsRng;
    let value = Secp256k1ScalarField::random(&mut rng);
    let subtraction = value.sub(&value);
    assert_eq!(subtraction, Secp256k1ScalarField::ZERO)
}

#[test]
fn inverse_correctness() {
    let mut rng = OsRng;
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
