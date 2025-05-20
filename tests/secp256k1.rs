use crypto_bigint::rand_core::OsRng;
use scl_rs::math::{
    ec::{secp256k1::Secp256k1, EllipticCurve},
    field::secp256k1_scalar::Secp256k1ScalarField,
    ring::Ring,
};

#[test]
fn subtraction_validity() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);
    let sub = curv_elem.sub(&curv_elem);
    assert!(sub.is_point_at_infinity())
}

#[test]
fn add_and_dbl_compatibility() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);

    // AddPoint = CurvElem + CurvElem = 2 * CurvElem
    let add_point = curv_elem.add(&curv_elem);
    assert!(add_point.eq(&curv_elem.dbl()))
}

#[test]
fn add_and_scalar_mul_compatibility() {
    let mut rng = OsRng;
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

#[test]
fn generator_is_valid() {
    assert!(Secp256k1::gen().to_affine().is_valid());
}

#[test]
fn dbl_and_scalar_mul_compatibility() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);

    let add_point = curv_elem.scalar_mul(&(Secp256k1ScalarField::ONE + &Secp256k1ScalarField::ONE));
    assert!(add_point.eq(&curv_elem.dbl()))
}

#[test]
fn identity_scalar_mul() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);
    let mult = curv_elem.scalar_mul(&Secp256k1ScalarField::ONE);
    assert!(mult.eq(&curv_elem));
}

#[test]
fn zero_scalar_mul() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);
    assert!(!curv_elem.is_point_at_infinity());

    let mult = curv_elem.scalar_mul(&Secp256k1ScalarField::ZERO);
    assert!(mult.is_point_at_infinity());
}

#[test]
fn dbl_of_point_at_infinity() {
    assert!(Secp256k1::POINT_AT_INFINITY.dbl().is_point_at_infinity())
}

#[test]
fn scalar_mul_point_at_infinity() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    assert!(Secp256k1::POINT_AT_INFINITY
        .scalar_mul(&coeff)
        .is_point_at_infinity())
}

#[test]
fn point_at_infinity_is_checked_correctly() {
    assert!(Secp256k1::POINT_AT_INFINITY.is_point_at_infinity())
}

#[test]
fn addition_between_two_zeros() {
    assert!(Secp256k1::POINT_AT_INFINITY
        .add(&Secp256k1::POINT_AT_INFINITY)
        .is_point_at_infinity())
}

#[test]
fn elliptic_curve_identity() {
    let mut rng = OsRng;
    let coeff = Secp256k1ScalarField::random_non_zero(&mut rng);
    let curv_elem = Secp256k1::gen().scalar_mul(&coeff);

    assert!(curv_elem.add(&Secp256k1::POINT_AT_INFINITY).eq(&curv_elem));
}
