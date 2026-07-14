//! This module contains implementation for different secret sharing schemes. The secret sharing
//! schemes currently supported are:
//!
//! - Additive secret sharing scheme,
//! - Feldman secret sharing scheme, and
//! - Shamir secret sharing scheme.
//!
//! For more information about how the schemes work, please refer to each module.
//!
//! The additive and Shamir schemes are *linear*: they implement the [`LinearShare`](crate::ss::LinearShare) trait, which
//! exposes the local, communication-free operations MPC protocols build on — adding two shares, and
//! adding, subtracting, or multiplying a share by a public constant — so a protocol can be written
//! generically over any linear scheme.

/// Implements additive secret sharing scheme.
pub mod additive;

/// Implements Feldman secret sharing scheme.
pub mod feldman;

/// Implements Shamir secret sharing scheme.
pub mod shamir;

use std::ops::{Add, Mul, Neg, Sub};

use crate::{math::ring::Ring, net::PartyId};
use rand::CryptoRng;

use super::math::poly;
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

/// Errors that occur when operating with shares.
#[derive(Debug, Error)]
#[non_exhaustive]
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
    /// The number of party indexes is different to the number of shares.
    #[error("the number of parties does not match with the number of shares - parties: {parties_idx_len}, shares: {shares_len}")]
    LengthMismatch {
        /// Length of party indexes.
        parties_idx_len: usize,
        /// Length of shares.
        shares_len: usize,
    },
    /// The share is not valid.
    #[error("invalid share from party {party_idx:?}")]
    InvalidShare {
        /// The index of the party owning the invalid share.
        party_idx: T,
    },
    /// One of the parties has index zero when computing the shares.
    #[error("one of the parties has index zero when computing the shares")]
    ZeroPartyId,
    /// The dealing threshold cannot be satisfied by the given number of parties — e.g. a Shamir
    /// polynomial degree so high that not even all the dealt shares could reconstruct the secret.
    #[error("invalid reconstruction threshold {threshold} for {n_parties} parties")]
    InvalidThreshold {
        /// The threshold requested by the caller (for Shamir, the polynomial degree).
        threshold: usize,
        /// The number of parties the secret was dealt to.
        n_parties: usize,
    },
}

/// A share in a **linear secret sharing scheme**.
///
/// A secret sharing scheme is *linear* when a party can combine the shares it holds — using the
/// scheme's own arithmetic and public constants — to obtain a share of a linear combination of the
/// underlying secrets, **without any communication between the parties**. This local homomorphism
/// is what MPC protocols build on: additions, subtractions and multiplications by public constants
/// are free (no rounds), while only multiplying two secret-shared values requires interaction.
///
/// Writing `[x]` for one party's share of a secret `x` in [`Value`](LinearShare::Value) and `c` for
/// a public constant, every implementor supports these local operations, expressed as the
/// [`Add`], [`Sub`], [`Mul`] and [`Neg`] bounds on the trait:
///
/// | Expression   | Operation                    | Resulting share |
/// | ------------ | ---------------------------- | --------------- |
/// | `&[x] + &[y]` | add two shares               | `[x + y]`       |
/// | `&[x] - &[y]` | subtract two shares          | `[x - y]`       |
/// | `-[x]`        | negate a share               | `[-x]`          |
/// | `&[x] + &c`   | add a public constant        | `[x + c]`       |
/// | `&[x] - &c`   | subtract a public constant   | `[x - c]`       |
/// | `&[x] * &c`   | multiply by a public scalar  | `[c · x]`       |
///
/// **Multiplying two shares (`[x] · [y]`) is deliberately *not* part of this trait**: it is not a
/// linear operation (in polynomial-based schemes it doubles the sharing degree) and cannot be done
/// locally, so it is provided by a separate interactive protocol (e.g. Beaver multiplication)
/// rather than an operator.
///
/// Dealing is parameterized by the scheme's [`Threshold`](LinearShare::Threshold): schemes with a
/// caller-chosen reconstruction threshold expose it there (Shamir uses the polynomial degree),
/// while schemes whose threshold is structural use `()` — additive sharing always needs every
/// share, so there is nothing for the caller to choose.
///
/// The trait is implemented on the *share* type — the single value a party holds — so a protocol
/// written generically over `S: LinearShare` runs unchanged on any linear scheme. The built-in
/// implementors are [`ShamirSS`](shamir::ShamirSS) and [`AdditiveSS`](additive::AdditiveSS); other
/// linear schemes (e.g. replicated secret sharing) can be added by implementing this trait.
///
/// This module covers only the *local* side of a shared computation. The interactive ends —
/// distributing shares from a dealer over the network and opening a shared secret — are provided
/// by the generic protocols in [`crate::protocol::share`], themselves written over
/// `S: LinearShare`.
///
/// Throughout, `shares` and `parties` are **positional**: `shares[i]` is the share held by
/// `parties[i]`, and both slices must have the same length.
///
/// # Examples
///
/// Code written against `S: LinearShare` runs over any linear scheme. For instance, an affine
/// combination `a · [x] + b` of a share with public constants — computed locally, no communication:
///
/// ```
/// use scl_rs::ss::LinearShare;
///
/// fn affine<S: LinearShare>(share: S, a: &S::Value, b: &S::Value) -> S {
///     share * a + b
/// }
/// ```
pub trait LinearShare:
    Sized
    + Send
    + Sync
    + Clone
    + Serialize
    + DeserializeOwned
    + for<'a> Add<&'a Self, Output = Self>
    + for<'a> Add<&'a Self::Value, Output = Self>
    + for<'a> Sub<&'a Self, Output = Self>
    + for<'a> Sub<&'a Self::Value, Output = Self>
    + for<'a> Mul<&'a Self::Value, Output = Self>
    + for<'a> Neg<Output = Self>
{
    /// The secret domain: the ring (or field) the shared value and the public constants live in.
    type Value: Ring;

    /// The scheme's reconstruction-threshold parameter for dealing.
    ///
    /// Schemes with a caller-chosen threshold expose it here: for Shamir this is the polynomial
    /// **degree** `t` — any `t + 1` shares reconstruct. Schemes whose threshold is structural use
    /// `()`: additive sharing always requires **all** shares, so there is nothing to choose — and
    /// no parameter to silently ignore.
    type Threshold: Copy + Send + Sync;

    /// Maps a party to its point in [`Value`](LinearShare::Value).
    ///
    /// Some schemes locate each party at a distinct point of the secret domain — Shamir sharing,
    /// for instance, evaluates the sharing polynomial at a unique x-coordinate per party. This is
    /// the canonical, scheme-defined encoding of that point. Because
    /// [`shares_from_secret`](LinearShare::shares_from_secret) and
    /// [`secret_from_shares`](LinearShare::secret_from_shares) both call it internally, the same
    /// mapping is always used to deal and to reconstruct, so the two can never disagree. Schemes
    /// that do not place parties in the field (e.g. additive sharing) never consult it and may
    /// return any value.
    ///
    /// Implementations must be **injective** (distinct parties map to distinct points) and must
    /// never map a party to the zero element ([`Ring::ZERO`]), which polynomial schemes reserve for
    /// the secret itself.
    fn encode_party(party: PartyId) -> Self::Value;

    /// Reconstructs the secret from a full set of shares.
    ///
    /// Combines the `shares` held by the corresponding `parties` back into the secret they encode,
    /// using [`encode_party`](LinearShare::encode_party) to place each party in the field. The two
    /// slices are positional (`shares[i]` belongs to `parties[i]`) and must have equal length; the
    /// set of shares must be large enough for the scheme to reconstruct (for a threshold scheme, at
    /// least `t + 1`). This is the inverse of
    /// [`shares_from_secret`](LinearShare::shares_from_secret): sharing a secret and then
    /// reconstructing from a valid subset yields the original value.
    ///
    /// # Errors
    ///
    /// Returns a [`ShareError`] if the shares are inconsistent or insufficient to reconstruct — for
    /// example mismatched `shares`/`parties` lengths, shares of differing degree, or fewer than the
    /// scheme's reconstruction threshold.
    fn secret_from_shares(
        shares: &[Self],
        parties: &[PartyId],
    ) -> Result<Self::Value, ShareError<Self::Value>>;

    /// Splits `secret` into one share per party.
    ///
    /// Returns a share for each entry of `parties`, positionally: the `i`-th returned share belongs
    /// to `parties[i]`, placed in the field via [`encode_party`](LinearShare::encode_party).
    /// `threshold` is the scheme's [`Threshold`](LinearShare::Threshold) parameter — the polynomial
    /// degree for Shamir, `()` for additive sharing. Any qualifying subset of the shares
    /// reconstructs the secret via [`secret_from_shares`](LinearShare::secret_from_shares).
    ///
    /// The sharing is randomized: the hiding randomness is drawn from `rng`, so a seeded caller
    /// gets reproducible dealings, and the [`CryptoRng`] bound keeps secret material from being
    /// derived from a predictable (non-cryptographic) generator.
    ///
    /// # Errors
    ///
    /// Returns a [`ShareError`] if `threshold` cannot be satisfied by `parties` — e.g.
    /// [`ShareError::InvalidThreshold`] for a Shamir degree that `parties.len()` shares could
    /// never reconstruct.
    fn shares_from_secret<R: CryptoRng>(
        secret: Self::Value,
        parties: &[PartyId],
        threshold: Self::Threshold,
        rng: &mut R,
    ) -> Result<Vec<Self>, ShareError<Self::Value>>;
}
