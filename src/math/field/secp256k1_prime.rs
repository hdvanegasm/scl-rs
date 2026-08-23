use crate::math::field::fp::Fp;
use crypto_bigint::{const_monty_params, Uint, U256};

const_monty_params!(
    Secp256k1PrimeFieldModulus,
    U256,
    "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F",
    "Modulus of the secp256k1 base field, that is, the prime $2^{256} - 2^{32} - 977$."
);

/// Represents an element in the field in which secp256k1 is defined.
pub type Secp256k1PrimeField = Fp<Secp256k1PrimeFieldModulus, { U256::LIMBS }>;

impl From<u64> for Secp256k1PrimeField {
    fn from(value: u64) -> Self {
        Fp::new(Uint::from_u64(value))
    }
}
