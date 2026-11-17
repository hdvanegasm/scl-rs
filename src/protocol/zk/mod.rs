//! Zero-knowledge proofs of knowledge, run as interactive two-party protocols.
//!
//! Each proof here is a [`Protocol`] executed by exactly two nodes: a
//! prover, who holds a secret witness, and a verifier, who holds only the public statement. The
//! verifier's output says whether it accepted the proof; the prover has no output.
//!
//! The proofs fail with [`Error`](crate::protocol::zk::Error). A proof the verifier rejects is not an error: it is
//! the verifier's output.

use crate::net::NetworkError;

/// Schnorr's interactive proof of knowledge of a discrete logarithm over an elliptic curve.
pub mod schnorr;

/// Error of the zero-knowledge proofs of this module.
//
// The derive is spelled `thiserror::Error` rather than imported, so that `Error` in this module
// names only this enum and the doc links to it stay unambiguous.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A network operation failed: sending, receiving, or encoding or decoding a packet.
    #[error("network error: {0:?}")]
    Network(#[from] NetworkError),
    /// The node does not fit its role: the prover and the verifier are the same party, the local
    /// party is neither of them, or the witness is missing on the prover or present on the
    /// verifier.
    #[error("the input is not well formed for the current protocol")]
    Input,
}
