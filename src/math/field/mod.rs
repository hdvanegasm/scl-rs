use std::{fmt::Debug, ops::Div};

use crate::math::ring;
use thiserror::Error;

/// This module contains an implementation of the field Mersenne 61 which is the
/// finite field of integers modulo $2^61 - 1$.
pub mod mersenne61;

/// Errors for mathematical operations between field elements.
#[derive(Error, Debug)]
pub enum FieldError {
    /// This error is raised when there is an inversion of zero.
    #[error("trying to compute the inverse of zero")]
    ZeroInverse,
}

/// Trait that represent a finite field of integers modulo a prime $p$.
pub trait FiniteField: ring::Ring + for<'a> Div<&'a Self> {
    /// Modulus used in for the field.
    const MODULUS: u64;

    /// Computes the inverse of field element.
    fn inverse(&self) -> Result<Self, FieldError>;
}
