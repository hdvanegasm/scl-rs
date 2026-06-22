//! In an additive secret sharing scheme for `n` parties is such that for a secret `x`, the shares of `x` are random
//! elements `[x_1, x_2, ..., x_n]` such that `x = x_1 + x_2 + ... + x_n`. In this secret sharing scheme,
//! the party `i` receives the share `x_i`.

use std::ops::Add;

use crate::math::ring::Ring;
use rand::CryptoRng;
use serde::{Deserialize, Serialize};

/// Represents an additive share.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdditiveSS<T>(T);

impl<T> AdditiveSS<T>
where
    T: Ring,
{
    /// Creates a new share for a ring element `value`.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Returns the value of the share as a ring element.
    pub fn share(&self) -> &T {
        &self.0
    }

    /// Computes the shares for a `secret` for `n_parties` number of parties using the CSPRNG `rng`.
    ///
    /// The shares are secret material, so `rng` is bound on [`CryptoRng`] to keep callers from
    /// seeding secrets with a predictable (non-cryptographic) generator. Pass a cryptographically
    /// secure source such as `rand::rng()` or a `ChaCha20Rng` seeded from OS entropy.
    pub fn shares_from_secret<R: CryptoRng>(secret: T, n_parties: usize, rng: &mut R) -> Vec<Self> {
        let mut shares = Vec::with_capacity(n_parties);
        let mut rand_acc = T::ZERO;
        for _ in 1..n_parties {
            let rnd_ring_value = T::random(rng);
            rand_acc = rand_acc + &rnd_ring_value;
            shares.push(Self(rnd_ring_value));
        }
        let last_share = Self(secret - &rand_acc);
        shares.push(last_share);
        shares
    }

    /// Computes a secret from an array of shares.
    pub fn secret_from_shares(shares: &[Self]) -> T {
        shares
            .iter()
            .fold(T::ZERO, |acc, share| acc + share.share())
    }
}

impl<T> Add<&Self> for AdditiveSS<T>
where
    T: Ring,
{
    type Output = Self;

    fn add(self, rhs: &Self) -> Self::Output {
        Self(self.0 + &rhs.0)
    }
}
