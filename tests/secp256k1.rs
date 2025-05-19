use crypto_bigint::rand_core::OsRng;
use scl_rs::math::{
    ec::{secp256k1::Secp256k1, EllipticCurve},
    field::secp256k1_scalar::Secp256k1ScalarField,
    ring::Ring,
};

#[test]
fn subtraction_validity() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);
    let sub = curv_elem.sub(&curv_elem);
    assert!(sub.is_point_at_infinity())
}

#[test]
fn addition_dbl_compatibility() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);
    let add_point = curv_elem.add(&curv_elem);
    assert!(add_point.eq(&curv_elem.dbl()))
}

#[test]
fn dbl_scalar_mul_compatibility() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);

    let add_point = curv_elem.scalar_mul(&(Secp256k1ScalarField::ONE + &Secp256k1ScalarField::ONE));
    assert!(add_point.eq(&curv_elem.dbl()))
}

#[test]
fn identity_scalar_mul() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);
    let mult = curv_elem.scalar_mul(&Secp256k1ScalarField::ONE);
    assert!(mult.eq(&curv_elem));
}

#[test]
fn zero_scalar_mul() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);
    let mult = curv_elem.scalar_mul(&Secp256k1ScalarField::ZERO);
    assert!(mult.is_point_at_infinity());
}
