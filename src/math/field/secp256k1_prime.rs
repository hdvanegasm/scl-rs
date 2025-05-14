use std::ops::{Add, Div, Mul, Sub};

use crypto_bigint::{ConstZero, Uint};
use serde::{Deserialize, Serialize};

use crate::math::ring::Ring;

use super::{FieldError, FiniteField};

const LIMBS: usize = 4;

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Hash, Eq, Serialize, Deserialize)]
pub struct Secp256k1PrimeField(Uint<LIMBS>);

impl FiniteField<LIMBS> for Secp256k1PrimeField {
    // TODO: Fix this number.
    const MODULUS: crypto_bigint::Uint<4> = Uint::from_words([
        0xFFFFFFFEFFFFFC2F,
        0xFFFFFFFFFFFFFFFF,
        0xFFFFFFFFFFFFFFFF,
        0xFFFFFFFFFFFFFFFF,
    ]);

    fn inverse(&self) -> Result<Self, super::FieldError> {
        todo!()
    }
}

impl Ring for Secp256k1PrimeField {
    const BIT_SIZE: usize = 4 * u64::BITS as usize;
    const ZERO: Self = Self(Uint::ZERO);
    const ONE: Self = Self(Uint::ONE);

    fn negate(&self) -> Self {
        todo!()
    }

    fn random<R: rand::Rng>(generator: &mut R) -> Self {
        todo!()
    }
}

impl Add<&Self> for Secp256k1PrimeField {
    type Output = Self;

    fn add(self, other: &Self) -> Self::Output {
        todo!()
    }
}

impl Sub<&Self> for Secp256k1PrimeField {
    type Output = Self;

    fn sub(self, other: &Self) -> Self::Output {
        todo!()
    }
}

impl Mul<&Self> for Secp256k1PrimeField {
    type Output = Self;

    fn mul(self, other: &Self) -> Self::Output {
        todo!()
    }
}

impl Div<&Self> for Secp256k1PrimeField {
    type Output = Result<Self, FieldError>;
    fn div(self, rhs: &Self) -> Self::Output {
        todo!()
    }
}
