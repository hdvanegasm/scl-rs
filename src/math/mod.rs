//! This module contains all the mathematical utilities commonly used in cryptography, such as,
//! basic abstract algebra structures, matrices, vectors, polynomials, and elliptic curves.

/// This module contains the main traits and implementations of finite fields.
pub mod field;

/// This module contains the implementation of matrices over finite rings and
/// their operations.
pub mod matrix;

/// This module contains the implementation of polynomials over finite rings
/// and their operations.
pub mod poly;

/// This module defines the main traits and implementations of finite rings.
pub mod ring;

/// This module contains the implementation of vectors over finite rings and
/// their operations.
pub mod vector;

// This module contains the implementation of some elliptic curves used in cryptography.
pub mod ec;
