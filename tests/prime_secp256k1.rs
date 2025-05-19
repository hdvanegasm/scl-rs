use crypto_bigint::rand_core::OsRng;
use scl_rs::math::{
    field::{secp256k1_prime::Secp256k1PrimeField, FiniteField},
    ring::Ring,
};
use std::ops::{Add, Div, Mul, Sub};

#[test]
fn subtraction_validity() {
    let mut rng = OsRng;
    let value = Secp256k1PrimeField::random(&mut rng);
    let subtraction = value.sub(&value);
    assert_eq!(subtraction, Secp256k1PrimeField::ZERO)
}

#[test]
fn inverse() {
    let mut rng = OsRng;
    let value = Secp256k1PrimeField::random(&mut rng);
    let inverse = value.inverse().unwrap();
    let mult = value.mul(&inverse);
    assert_eq!(mult, Secp256k1PrimeField::ONE);
}
