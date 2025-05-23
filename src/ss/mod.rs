pub mod additive;
pub mod feldman;
pub mod shamir;

use crate::math::ring::Ring;

use super::math::poly;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ShareError<T: Ring> {
    #[error("there are not enough shares to reconstruct the secret")]
    NotEnoughShares,

    #[error("the shares do not have the same degree")]
    SharesWithDifferentDegree,

    #[error("error during the share reconstruction {0:?}")]
    ReconstructionError(poly::Error<T>),
}
