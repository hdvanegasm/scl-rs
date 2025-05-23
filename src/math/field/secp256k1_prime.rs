use super::{FieldError, FiniteField};
use crate::math::ring::Ring;
use crypto_bigint::{rand_core::RngCore, NonZero, RandomMod, Uint, Zero};
use serde::{Deserialize, Serialize};
use std::ops::{Add, Div, Mul, Sub};

#[derive(Debug, Copy, Clone, PartialEq, Hash, Eq, Serialize, Deserialize)]
pub struct Secp256k1PrimeField(Uint<4>);

impl Secp256k1PrimeField {
    pub fn new(value: Uint<4>) -> Self {
        Self(value)
    }
}

impl FiniteField<4> for Secp256k1PrimeField {
    const MODULUS: NonZero<Uint<4>> = NonZero::<Uint<4>>::new_unwrap(Uint::from_words([
        0xFFFFFFFEFFFFFC2F,
        0xFFFFFFFFFFFFFFFF,
        0xFFFFFFFFFFFFFFFF,
        0xFFFFFFFFFFFFFFFF,
    ]));

    fn inverse(&self) -> Result<Self, FieldError> {
        if bool::from(self.0.is_zero()) {
            Err(FieldError::ZeroInverse)
        } else {
            // SAFETY: This unwrap is safe as rhs is non-zero.
            let inverse = self.0.inv_mod(&Self::MODULUS).unwrap();
            Ok(Self(inverse))
        }
    }
}

impl From<u64> for Secp256k1PrimeField {
    fn from(value: u64) -> Self {
        Self(Uint::<4>::from_u64(value))
    }
}

impl Ring for Secp256k1PrimeField {
    const BIT_SIZE: usize = Self::LIMBS * u64::BITS as usize;
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

impl Add<&Self> for Secp256k1PrimeField {
    type Output = Self;

    fn add(self, other: &Self) -> Self::Output {
        Self(self.0.add_mod(&other.0, &Self::MODULUS))
    }
}

impl Sub<&Self> for Secp256k1PrimeField {
    type Output = Self;

    fn sub(self, other: &Self) -> Self::Output {
        Self(self.0.sub_mod(&other.0, &Self::MODULUS))
    }
}

impl Mul<&Self> for Secp256k1PrimeField {
    type Output = Self;

    fn mul(self, other: &Self) -> Self::Output {
        Self(self.0.mul_mod(&other.0, &Self::MODULUS))
    }
}

impl Div<&Self> for Secp256k1PrimeField {
    type Output = Result<Self, FieldError>;
    fn div(self, rhs: &Self) -> Self::Output {
        let inverse = rhs.inverse()?;
        Ok(self.mul(&inverse))
    }
}
