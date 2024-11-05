use std::{
    fmt::Debug,
    ops::{Add, Mul, Sub},
};

use rand::Rng;
use serde::{Deserialize, Serialize};

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
{
    /// Type of the underlying representation for a field element.
    type ValueType;

    /// Number of bits of each element.
    const BIT_SIZE: usize;

    /// Additive identity of the field.
    const ZERO: Self;

    /// Multiplicative identity of the field.
    const ONE: Self;

    /// Computes the additive inverse of a field element.
    fn negate(&self) -> Self;

    /// Generates a random finite field element with a provided pseudo-random generator.
    fn random<R: Rng>(generator: &mut R) -> Self;
}
