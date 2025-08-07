pub mod additive;
pub mod feldman;
pub mod shamir;

use crate::math::ring::Ring;

use super::math::poly;
use thiserror::Error;

/// Errors that occur when operating with shares.
#[derive(Debug, Error)]
pub enum ShareError<T: Ring> {
    /// Error when trying to reconstruct a secret but there are not enough shares.
    #[error("there are not enough shares to reconstruct the secret")]
    NotEnoughShares,
    /// Error that arises in Shamir secret sharing when the shares do not have the same degree.
    #[error("the shares do not have the same degree")]
    SharesWithDifferentDegree,
    #[error("error during the share reconstruction {0:?}")]
    ReconstructionError(poly::Error<T>),
}
