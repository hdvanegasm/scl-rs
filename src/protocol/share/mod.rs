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

/// Distributing shares of a secret from a designated dealer.
pub mod deal;

/// Opening (reconstructing) a shared secret by exchanging shares.
pub mod open;
