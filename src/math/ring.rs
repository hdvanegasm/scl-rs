use std::{
    fmt::Debug,
    hash::Hash,
    ops::{Add, Mul, Sub},
};

use rand::Rng;
use serde::{Deserialize, Serialize};

/// This trait represent an algebraic finite Ring.
pub trait Ring:
    Debug
    + PartialEq
    + Eq
    + Sized
    + Clone
    + From<u64>
    + Serialize
    + for<'a> Deserialize<'a>
    + for<'a> Add<&'a Self, Output = Self>
    + for<'a> Mul<&'a Self, Output = Self>
    + for<'a> Sub<&'a Self, Output = Self>
    + Copy
    + Hash
{
    /// Type of the underlying representation for a ring element.
    type ValueType;

    /// Number of bits of each element.
    const BIT_SIZE: usize;

    /// Additive identity of the ring.
    const ZERO: Self;

    /// Multiplicative identity of the ring.
    const ONE: Self;

    /// Computes the additive inverse of a ring element.
    fn negate(&self) -> Self;

    /// Generates a random finite ring element with a provided pseudo-random generator.
    fn random<R: Rng>(generator: &mut R) -> Self;
}
