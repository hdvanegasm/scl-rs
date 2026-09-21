use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    hash::Hash,
    ops::{Add, Mul, Neg, Sub},
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
    + Neg<Output = Self>
    + Copy
    + Hash
{
    /// Bit size of the representation of a ring element, that is, the number of bits the element
    /// occupies when it is stored or sent over the network.
    ///
    /// This is the width of the representation, not the width of the largest value: for a ring of
    /// integers modulo $m$ the elements are stored in whole limbs, so `BIT_SIZE` is a multiple of
    /// the limb size and is at least $\lceil \log_2 m \rceil$.
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
    fn random<R: Rng>(generator: &mut R) -> Self;

    /// Returns a non-zero element from the ring sampled uniformly at random.
    fn random_non_zero<R: Rng>(generator: &mut R) -> Self;

    /// Computes the power of a ring with an exponent.
    ///
    /// The maximum possible exponent is $2^{64}$ which is enough for common cryptography
    /// applications. This algorithm was adapted from the [num-bigint](https://github.com/rust-num/num-bigint/blob/master/src/biguint/power.rs) crate.
    fn pow(&self, mut exp: u64) -> Self {
        if exp == 0 {
            Self::ONE
        } else {
            let mut base = *self;

            while exp & 1 == 0 {
                base = base.mul(&base);
                exp >>= 1;
            }

            if exp == 1 {
                return base;
            }

            let mut acc = base;
            while exp > 1 {
                exp >>= 1;
                base = base.mul(&base);
                if exp & 1 == 1 {
                    acc = acc * &base;
                }
            }
            acc
        }
    }
}
