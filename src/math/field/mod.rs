use crate::math::ring;
use crypto_bigint::{NonZero, Uint};
use std::{fmt::Debug, ops::Div};
use thiserror::Error;

/// This module contains an implementation of the field Mersenne 61 which is the
/// finite field of integers modulo $2^61 - 1$.
pub mod mersenne61;

pub mod secp256k1_prime;

pub mod secp256k1_scalar;

pub mod naf;

/// Errors for mathematical operations between field elements.
#[derive(Error, Debug)]
pub enum FieldError {
    /// This error is raised when there is an inversion of zero.
    #[error("trying to compute the inverse of zero")]
    ZeroInverse,
}

/// Trait that represent a finite field of integers modulo a prime $p$.
pub trait FiniteField<const LIMBS: usize>:
    ring::Ring + for<'a> Div<&'a Self> + Copy + Clone
{
    /// Modulus used in for the field represented in Little-Endian.
    const MODULUS: NonZero<Uint<LIMBS>>;

    /// Computes the inverse of field element.
    fn inverse(&self) -> Result<Self, FieldError>;
}
