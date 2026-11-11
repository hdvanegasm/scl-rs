//! Zero-knowledge proofs of knowledge, run as interactive two-party protocols.
//!
//! Each proof here is a [`Protocol`] executed by exactly two nodes: a
//! prover, who holds a secret witness, and a verifier, who holds only the public statement. The
//! verifier's output says whether it accepted the proof; the prover has no output.

/// Schnorr's interactive proof of knowledge of a discrete logarithm over an elliptic curve.
pub mod schnorr;
