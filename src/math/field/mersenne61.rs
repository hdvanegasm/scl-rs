use super::FieldError;
use super::FiniteField;
use crate::math::ring::Ring;
use crypto_bigint::rand_core::RngCore;
use crypto_bigint::NonZero;
use crypto_bigint::U64;
use serde::Deserialize;
use serde::Serialize;
use std::hash::Hash;
use std::ops::Add;
use std::ops::Div;
use std::ops::Mul;
use std::ops::Sub;

/// Representation of a field element modulo 2^{61} - 1.
#[derive(PartialEq, Eq, Copy, Serialize, Deserialize, Clone, Debug)]
pub struct Mersenne61(u64);

impl From<u64> for Mersenne61 {
    fn from(value: u64) -> Self {
        let mut final_value = value;
        while final_value >= u64::from(Self::MODULUS.to_limbs()[0]) {
            final_value -= u64::from(Self::MODULUS.to_limbs()[0]);
        }
        Self(final_value)
    }
}

impl Ring for Mersenne61 {
    const BIT_SIZE: usize = 61;
    const ONE: Self = Self(1);
    const ZERO: Self = Self(0);
    const LIMBS: usize = 1;

    fn random<R: RngCore>(generator: &mut R) -> Self {
        let value: u64 = generator.next_u64();
        Self::from(value)
    }

    fn random_non_zero<R: RngCore>(generator: &mut R) -> Self {
        let mut value = generator.next_u64();
        while value == 0 {
            value = generator.next_u64();
        }
        Self::from(value)
    }

    fn negate(&self) -> Self {
        if !self.eq(&Self::ZERO) {
            Self::from(u64::from(Self::MODULUS.to_limbs()[0]) - self.0)
        } else {
            Self::ZERO
        }
    }
}

impl Add<&Self> for Mersenne61 {
    type Output = Self;

    fn add(self, other: &Self) -> Self::Output {
        let add_result = self.0 + other.0;
        Self::from(add_result)
    }
}

impl Sub<&Self> for Mersenne61 {
    type Output = Self;

    fn sub(self, other: &Self) -> Self::Output {
        if other.0 > self.0 {
            Self::from(self.0 + u64::from(Self::MODULUS.to_limbs()[0]) - other.0)
        } else {
            Self::from(self.0 - other.0)
        }
    }
}

impl Mul<&Self> for Mersenne61 {
    type Output = Self;

    fn mul(self, other: &Self) -> Self::Output {
        let non_reduced_mult: u128 = (self.0 as u128) * (other.0 as u128);
        let mut most_sig_bits = (non_reduced_mult >> Self::BIT_SIZE) as u64;
        let mut least_sig_bits = non_reduced_mult as u64;

        most_sig_bits |= least_sig_bits >> Self::BIT_SIZE;
        least_sig_bits &= u64::from(Self::MODULUS.as_limbs()[0]);

        // Apply modular addition.
        let most_sig_bits_mod = Self::from(most_sig_bits);
        let least_sig_bits_mod = Self::from(least_sig_bits);
        most_sig_bits_mod.add(&least_sig_bits_mod)
    }
}

impl Div<&Self> for Mersenne61 {
    type Output = Result<Self, FieldError>;
    fn div(self, rhs: &Self) -> Self::Output {
        let inverse = self.inverse()?;
        Ok(rhs.mul(&inverse))
    }
}

impl FiniteField<1> for Mersenne61 {
    const MODULUS: NonZero<U64> = NonZero::<U64>::new_unwrap(U64::from_u64(0x1FFFFFFFFFFFFFFF));

    fn inverse(&self) -> Result<Self, super::FieldError> {
        if self.eq(&Self::ZERO) {
            Err(FieldError::ZeroInverse)
        } else {
            let mut k: i64 = 0;
            let mut new_k: i64 = 1;
            let mut r: i64 = u64::from(Self::MODULUS.to_limbs()[0]) as i64;
            let mut new_r: i64 = self.0 as i64;

            while new_r != 0 {
                let q = r / new_r;
                assign(&mut k, &mut new_k, q);
                assign(&mut r, &mut new_r, q);
            }

            if k < 0 {
                k += u64::from(Self::MODULUS.to_limbs()[0]) as i64;
            }

            Ok(Self::from(k as u64))
        }
    }
}

impl Hash for Mersenne61 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Given v1, v2 and a constant q, computes the multiplicative exchange
/// v1 <- v2 and v2 <- v1 - q * v2.
fn assign(v1: &mut i64, v2: &mut i64, q: i64) {
    let temp = *v2;
    *v2 = *v1 - q * temp;
    *v1 = temp;
}
