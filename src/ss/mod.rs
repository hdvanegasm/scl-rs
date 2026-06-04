//! This module contains implementation for different secret sharing schemes. The secret sharing
//! schemes currently supported are:
//!
//! - Additive secret sharing scheme,
//! - Feldman secret sharing scheme, and
//! - Shamir secret sharing scheme.
//!
//! For more information about how the schemes work, please refer to each module.

/// Implements additive secret sharing scheme.
pub mod additive;

/// Implements Feldman secret sharing scheme.
pub mod feldman;

/// Implements Shamir secret sharing scheme.
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
    /// There was an error during the reconstruction of a secret using Shamir secret sharing scheme.
    #[error("error during the share reconstruction {0:?}")]
    ReconstructionError(poly::Error<T>),
    /// The number of indexes for each party and the number of shares is different.
    #[error("the number of shares and evaluation points do not match. Evaluation points: {n_eval_points}, Shares: {n_shares}")]
    EvalAndShareLenMismatch {
        /// Number of evaluation points.
        n_eval_points: usize,
        /// Number of shares.
        n_shares: usize,
    },
    /// The number of shares is different to the number of shares.
    #[error("the number of parties does not match with the number of shares - parties: {parties_idx_len}, shares: {shares_len}")]
    LengthMismatch {
        /// Length of party indexes.
        parties_idx_len: usize,
        /// Length of shares.
        shares_len: usize,
    },
    /// The share is not valid.
    #[error("invalid share from party {party_idx}")]
    InvalidShare {
        /// The index of the party owning the invalid share.
        party_idx: T,
    },
    /// One of the parties has index zero when computing the shares.
    #[error("one of the parties has index zero when computing the shares")]
    ZeroPartyId,
}
