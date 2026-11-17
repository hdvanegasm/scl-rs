//! Protocols for moving secrets in and out of a linear secret sharing scheme.
//!
//! These protocols are written generically over [`LinearShare`](crate::ss::LinearShare), so the
//! same implementation runs over any linear scheme (Shamir, additive, ...). Together they cover
//! the two interactive ends of a shared computation — everything in between (linear arithmetic on
//! shares) is local and needs no protocol:
//!
//! - [`deal`](crate::protocol::share::deal): a designated dealer splits a secret and distributes
//!   one share to each receiver.
//! - [`open`](crate::protocol::share::open): the parties reveal their shares to reconstruct
//!   (open) the secret — either to everyone, or towards a single designated party.
//!
//! # Security model: passive adversary
//!
//! All protocols in this module assume a **passive (semi-honest) adversary** — reflected in their
//! `Passive*` names: every party follows the protocol, so a party that is expected to send a
//! share always does, and blocking on it is safe. Parties that crash or withhold messages are
//! outside this model and stall the protocol. Lifting the assumption — receive timeouts on the
//! network layer and malicious-model (identifiable-abort) variants of these protocols — is
//! planned follow-on work; see `docs/roadmap.md` §11.
//!
//! # Errors
//!
//! The protocols here fail with [`Error<T>`](crate::protocol::share::Error), where `T` is the type of the shared
//! values: `S::Value` for a scheme `S`, so the field `F` for
//! [`ShamirSS<LIMBS, F>`](crate::ss::shamir::ShamirSS). The [DN07
//! protocols](crate::protocol::passive_shamir) fail with the same type, so that they and the
//! protocols here can call one another and run in one [`Join`](crate::protocol::concurrent::Join).

use crate::{math::ring::Ring, net::NetworkError, ss::ShareError};

/// Distributing shares of a secret from a designated dealer.
pub mod deal;

/// Opening (reconstructing) a shared secret by exchanging shares.
pub mod open;

/// Error of the secret-sharing protocols: those of this module and of
/// [`passive_shamir`](crate::protocol::passive_shamir).
///
/// `T` is the type of the shared values, the field `F` for Shamir sharings over `F`. Keeping it
/// as a type parameter lets [`Share`](Error::Share) hold the structured [`ShareError<T>`], so a
/// caller matches on it directly.
//
// The derive is spelled `thiserror::Error` rather than imported, so that `Error` in this module
// names only this enum and the doc links to it stay unambiguous.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error<T: Ring> {
    /// A network operation failed: sending, receiving, or encoding or decoding a packet.
    #[error("network error: {0:?}")]
    Network(#[from] NetworkError),
    /// Dealing or reconstructing a secret failed.
    #[error("share error: {0}")]
    Share(#[from] ShareError<T>),
    /// The input does not fit the protocol: for example, running as the dealer of
    /// [`PassiveDealShr`](deal::PassiveDealShr) without a secret, or constructing a
    /// [`passive_shamir`](crate::protocol::passive_shamir) protocol with a corruption threshold
    /// too large for its parties, or with input vectors of different lengths.
    #[error("the input is not well formed for the current protocol")]
    Input,
}
