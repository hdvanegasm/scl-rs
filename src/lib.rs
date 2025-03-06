//! **scl-rs** is a set of tools to quickly implement MPC protocols in a
//! distributed way. The main features of *scl-rs* are:
//! - A functional network library that uses TLS (using *rustls*) for the communication between parties.
//! - A set of mathematical tools that are common to MPC protocols:
//!     - Fields.
//!     - Rings.
//!     - Polynomial over rings and their operations.
//!     - Polynomial interpolation over fields.
//!     - Matrices and vectors over rings.
//!     - Finite field implementations.
//!     - Elliptic curve implementations (work in progress).
//! - A set of MPC facilities that are common to a wide variety of protocols (work in progress):
//!     - Feldman verifiable secret-sharing.
//!     - Shamir secret-sharing.
//!     - Additive secret sharing.

/// This module contains all the mathematical tools used in MPC protocols.
pub mod math;

/// This module contains all the network facilities and methods that allow a set of parties
/// to conect between them using TLS.
pub mod net;

/// This module contains the implementation of some tools commonly used in MPC protocols
/// based on secret-sharing techniques.
pub mod ss;
