use crypto_bigint::rand_core::OsRng;
use scl_rs::math::{field::secp256k1_prime::Secp256k1PrimeField, ring::Ring, vector::Vector};

#[test]
fn dot_with_zero() {
    let mut rng = OsRng;
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len);
    let dot_prod = vector.dot(&zero_vec);
    assert_eq!(dot_prod.unwrap(), Secp256k1PrimeField::ZERO);
}

#[test]
fn add_with_zero() {
    let mut rng = OsRng;
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len);
    let add_vec = &vector + &zero_vec;
    assert_eq!(add_vec.unwrap(), vector);
}

#[test]
#[should_panic]
fn incompatible_dot() {
    let mut rng = OsRng;
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    vector.dot(&zero_vec).unwrap();
}

#[test]
#[should_panic]
fn incompatible_add_with_zero() {
    let mut rng = OsRng;
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    let add_vec = &vector + &zero_vec;
    add_vec.unwrap();
}

#[test]
#[should_panic]
fn incompatible_sub_with_zero() {
    let mut rng = OsRng;
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    let sub = &vector - &zero_vec;
    sub.unwrap();
}

#[test]
#[should_panic]
fn incompatible_mul_with_zero() {
    let mut rng = OsRng;
    let len = 100;
    let vector = Vector::<Secp256k1PrimeField>::random(len, &mut rng);
    let zero_vec = Vector::zero(len + 1);
    let mul = &vector * &zero_vec;
    mul.unwrap();
}
