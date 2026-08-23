use crate::math::{
    field::{fp::Fp, naf::NafEncoding},
    ring::Ring,
};
use crypto_bigint::{const_monty_params, Limb, Uint, U256};

const_monty_params!(
    Secp256k1ScalarFieldModulus,
    U256,
    "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141",
    "Modulus of the secp256k1 scalar field, that is, the order of the curve's prime-order subgroup."
);

/// Represents a finite field modulo a secp256k1 prime order sub-group.
pub type Secp256k1ScalarField = Fp<Secp256k1ScalarFieldModulus, { U256::LIMBS }>;

impl From<u64> for Secp256k1ScalarField {
    fn from(value: u64) -> Self {
        Fp::new(Uint::from_u64(value))
    }
}

impl Secp256k1ScalarField {
    /// Computes the NAF representation of this field element.
    pub fn to_naf(&self) -> NafEncoding {
        let mut naf = NafEncoding::new(Self::BIT_SIZE + 1);
        let mut val = self.retrieve();
        let mut i = 0;

        while !bool::from(val.is_zero()) {
            if test_bit(val, 0) {
                if test_bit(val, 1) {
                    naf.create_neg(i);
                    val += Uint::ONE;
                } else {
                    naf.create_pos(i);
                    val -= Uint::ONE;
                }
            } else {
                naf.create_zero(i);
            }
            i += 1;
            val >>= 1;
        }
        naf
    }
}

/// Returns `true` if the bit in the position is 1, otherwise returns `false`.
fn test_bit<const LIMBS: usize>(input: Uint<LIMBS>, pos: usize) -> bool {
    assert!((pos as u32) < LIMBS as u32 * Limb::BITS);
    let bits_per_limb = Limb::BITS;

    let limbs_input = input.as_limbs();
    let limb = pos as u32 / bits_per_limb;
    let limb_pos = pos as u32 % bits_per_limb;
    ((limbs_input[limb as usize] >> limb_pos) & Limb::ONE) == Limb::ONE
}
