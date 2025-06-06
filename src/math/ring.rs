use crypto_bigint::rand_core::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    hash::Hash,
    ops::{Add, Mul, Sub},
};

/// This trait represent an algebraic finite Ring.
pub trait Ring:
    Debug
    + PartialEq
    + Eq
    + Sized
    + Clone
    + Serialize
    + for<'a> Deserialize<'a>
    + for<'a> Add<&'a Self, Output = Self>
    + for<'a> Mul<&'a Self, Output = Self>
    + for<'a> Sub<&'a Self, Output = Self>
    + Copy
    + Hash
{
    /// Bit size of an element of the ring.
    const BIT_SIZE: usize;

    /// Additive identity of the ring.
    const ZERO: Self;

    /// Number of limbs used to represent a ring element.
    const LIMBS: usize;

    /// Multiplicative identity of the ring.
    const ONE: Self;

    /// Computes the additive inverse of a ring element.
    fn negate(&self) -> Self;

    /// Generates a random finite ring element with a provided pseudo-random generator.
    fn random<R: RngCore>(generator: &mut R) -> Self;

    /// Returns a non-zero element from the ring sampled uniformly at random.
    fn random_non_zero<R: RngCore>(generator: &mut R) -> Self;
}
