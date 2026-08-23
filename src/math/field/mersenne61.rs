use crate::math::field::fp::Fp;
use crypto_bigint::{const_monty_params, Uint, U64};

const_monty_params!(
    Mersenne61Modulus,
    U64,
    "1FFFFFFFFFFFFFFF",
    "Modulus of the Mersenne 61 field, that is, the prime $2^{61} - 1$."
);

/// Representation of a field element modulo $2^{61} - 1$.
pub type Mersenne61 = Fp<Mersenne61Modulus, { U64::LIMBS }>;

impl From<u64> for Mersenne61 {
    fn from(value: u64) -> Self {
        Fp::new(Uint::from_u64(value))
    }
}
