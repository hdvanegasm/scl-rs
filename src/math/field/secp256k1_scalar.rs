use super::{naf::NafEncoding, FieldError, FiniteField};
use crate::math::ring::Ring;
use crypto_bigint::{rand_core::RngCore, Limb, NonZero, RandomMod, Uint, Zero};
use serde::{Deserialize, Serialize};
use std::ops::{Add, Div, Mul, Sub};

const LIMBS: usize = 4;

/// Represents a finite field modulo a secp256k1 prime order sub-group.
#[derive(Debug, Copy, Clone, PartialEq, Hash, Eq, Serialize, Deserialize)]
pub struct Secp256k1ScalarField(Uint<LIMBS>);

impl Secp256k1ScalarField {
    /// Computes the NAF representation of this field element.
    pub fn to_naf(&self) -> NafEncoding {
        let mut naf = NafEncoding::new(Self::BIT_SIZE + 1);
        let mut val = self.0;
        let mut i = 0;

        while !bool::from(val.is_zero()) {
            if test_bit(val, 0) {
                if test_bit(val, 1) {
                    naf.create_neg(i);
                    val += Uint::<4>::ONE;
                } else {
                    naf.create_pos(i);
                    val -= Uint::<4>::ONE;
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

impl FiniteField<4> for Secp256k1ScalarField {
    const MODULUS: NonZero<Uint<4>> = NonZero::<Uint<4>>::new_unwrap(Uint::from_words([
        0xBFD25E8CD0364141,
        0xBAAEDCE6AF48A03B,
        0xFFFFFFFFFFFFFFFE,
        0xFFFFFFFFFFFFFFFF,
    ]));

    fn inverse(&self) -> Result<Self, super::FieldError> {
        if bool::from(self.0.is_zero()) {
            Err(FieldError::ZeroInverse)
        } else {
            // SAFETY: This unwrap is safe as rhs is non-zero.
            let inverse = self.0.inv_mod(&Self::MODULUS).unwrap();
            Ok(Self(inverse))
        }
    }
}

impl From<u64> for Secp256k1ScalarField {
    fn from(value: u64) -> Self {
        Self(Uint::<4>::from_u64(value))
    }
}

impl Ring for Secp256k1ScalarField {
    const BIT_SIZE: usize = Self::LIMBS * Limb::BITS as usize;
    const ZERO: Self = Self(Uint::ZERO);
    const ONE: Self = Self(Uint::ONE);
    const LIMBS: usize = 4;

    fn negate(&self) -> Self {
        Self(self.0.neg_mod(&Self::MODULUS))
    }

    fn random<R: RngCore>(generator: &mut R) -> Self {
        let value = Uint::<4>::random_mod(generator, &Self::MODULUS);
        Self(value)
    }

    fn random_non_zero<R: RngCore>(generator: &mut R) -> Self {
        let mut value = Uint::<4>::random_mod(generator, &Self::MODULUS);
        while bool::from(value.is_zero()) {
            value = Uint::<4>::random_mod(generator, &Self::MODULUS);
        }
        Self(value)
    }
}

impl Add<&Self> for Secp256k1ScalarField {
    type Output = Self;

    fn add(self, other: &Self) -> Self::Output {
        Self(self.0.add_mod(&other.0, &Self::MODULUS))
    }
}

impl Sub<&Self> for Secp256k1ScalarField {
    type Output = Self;

    fn sub(self, other: &Self) -> Self::Output {
        Self(self.0.sub_mod(&other.0, &Self::MODULUS))
    }
}

impl Mul<&Self> for Secp256k1ScalarField {
    type Output = Self;

    fn mul(self, other: &Self) -> Self::Output {
        Self(self.0.mul_mod(&other.0, &Self::MODULUS))
    }
}

impl Div<&Self> for Secp256k1ScalarField {
    type Output = Result<Self, FieldError>;
    fn div(self, rhs: &Self) -> Self::Output {
        let inverse = rhs.inverse()?;
        Ok(self.mul(&inverse))
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
