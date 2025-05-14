use crate::math::field::{
    secp256k1_prime::Secp256k1PrimeField, secp256k1_scalar::Secp256k1ScalarField,
};

use super::EllipticCurve;

pub struct Secp256k1;

impl EllipticCurve for Secp256k1 {
    type ScalarField = Secp256k1ScalarField;
    type PrimeField = Secp256k1PrimeField;
}
