//! In an additive secret sharing scheme for `n` parties is such that for a secret `x`, the shares of `x` are random
//! elements `[x_1, x_2, ..., x_n]` such that `x = x_1 + x_2 + ... + x_n`. In this secret sharing scheme,
//! the party `i` receives the share `x_i`.

use std::ops::{Add, Mul, Neg, Sub};

use crate::{abbreviate::Abbreviate, math::ring::Ring, net::PartyId};
use rand::CryptoRng;
use serde::{Deserialize, Serialize};

use super::{LinearShare, ShareError};

/// Represents an additive share held by one party.
///
/// Besides its `value`, the share records the `party` that holds it and whether that party is the
/// **leader** — the single party that absorbs public constants. Because additive reconstruction is
/// the sum of all shares, adding a public constant `c` (see the [`Add<&T>`](AdditiveSS::add) /
/// [`Sub<&T>`](AdditiveSS::sub) operators) must change exactly one share, so only the leader's share
/// is adjusted; every other party leaves its share untouched. The leader is chosen at dealing time
/// as the party with the smallest id (see [`shares_from_secret`](AdditiveSS::shares_from_secret)).
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct AdditiveSS<T> {
    /// The value of this party's share.
    value: T,
    /// The party holding this share.
    party: PartyId,
    /// Whether this party is the leader that absorbs public constants.
    is_leader: bool,
}

impl<T> Abbreviate for AdditiveSS<T> {
    const ABBREVIATION: &'static str = "add. shr.";
}

impl<T> AdditiveSS<T>
where
    T: Ring,
{
    /// Creates a new share of `value` held by `party`, marking whether `party` is the leader that
    /// absorbs public constants.
    ///
    /// This is the low-level constructor; most callers deal shares from a secret with
    /// [`shares_from_secret`](AdditiveSS::shares_from_secret), which assigns the leader flag
    /// consistently across all shares.
    ///
    /// # Examples
    ///
    /// ```
    /// use scl_rs::math::field::mersenne61::Mersenne61;
    /// use scl_rs::net::PartyId;
    /// use scl_rs::ss::additive::AdditiveSS;
    ///
    /// // A share of `7` held by party 0, marked as the leader (the absorber of public constants).
    /// let share = AdditiveSS::new(Mersenne61::from(7u64), PartyId::from(0usize), true);
    /// assert_eq!(*share.share(), Mersenne61::from(7u64));
    /// assert_eq!(share.party(), PartyId::from(0usize));
    /// ```
    pub fn new(value: T, party: PartyId, is_leader: bool) -> Self {
        Self {
            value,
            party,
            is_leader,
        }
    }

    /// Returns the value of the share as a ring element.
    pub fn share(&self) -> &T {
        &self.value
    }

    /// Returns the party that holds this share.
    pub fn party(&self) -> PartyId {
        self.party
    }

    /// Computes the shares of a `secret`, one for each party in `parties`, using the CSPRNG `rng`.
    ///
    /// The `i`-th returned share belongs to `parties[i]`. The party with the smallest id is marked
    /// as the leader (the absorber of public constants); every share carries that decision so the
    /// public-constant operators are correct regardless of how the parties are numbered.
    ///
    /// The shares are secret material, so `rng` is bound on [`CryptoRng`] to keep callers from
    /// seeding secrets with a predictable (non-cryptographic) generator. Pass a cryptographically
    /// secure source such as `rand::rng()` or a `ChaCha20Rng` seeded from OS entropy.
    ///
    /// # Examples
    ///
    /// ```
    /// use scl_rs::math::field::mersenne61::Mersenne61;
    /// use scl_rs::net::PartyId;
    /// use scl_rs::ss::additive::AdditiveSS;
    ///
    /// let secret = Mersenne61::from(42u64);
    /// let parties: Vec<PartyId> = (0..3usize).map(PartyId::from).collect();
    ///
    /// let shares = AdditiveSS::shares_from_secret(secret, &parties, &mut rand::rng());
    /// assert_eq!(AdditiveSS::secret_from_shares(&shares), secret);
    ///
    /// // Adding a public constant is a local operation: only the leader (the smallest party id)
    /// // absorbs it, so the shares still sum to `secret + 8`.
    /// let eight = Mersenne61::from(8u64);
    /// let shifted: Vec<_> = shares.into_iter().map(|share| share + &eight).collect();
    /// assert_eq!(AdditiveSS::secret_from_shares(&shifted), secret + &eight);
    /// ```
    pub fn shares_from_secret<R: CryptoRng>(
        secret: T,
        parties: &[PartyId],
        rng: &mut R,
    ) -> Vec<Self> {
        let leader = parties.iter().copied().min();

        // Sample all but the last share uniformly at random; the last one makes the shares sum to
        // the secret.
        let mut rand_acc = T::ZERO;
        let mut values = Vec::with_capacity(parties.len());
        for _ in 1..parties.len() {
            let rnd_ring_value = T::random(rng);
            rand_acc = rand_acc + &rnd_ring_value;
            values.push(rnd_ring_value);
        }
        values.push(secret - &rand_acc);

        parties
            .iter()
            .zip(values)
            .map(|(party, value)| Self {
                value,
                party: *party,
                is_leader: Some(*party) == leader,
            })
            .collect()
    }

    /// Computes a secret from an array of shares by summing their values.
    pub fn secret_from_shares(shares: &[Self]) -> T {
        shares
            .iter()
            .fold(T::ZERO, |acc, share| acc + share.share())
    }
}

// --- Local (communication-free) linear operations. See [`LinearShare`] for their MPC meaning. ---

impl<T: Ring> Add<&Self> for AdditiveSS<T> {
    type Output = Self;

    /// Adds two shares held by the same party: `[x] + [y] = [x + y]`.
    fn add(self, rhs: &Self) -> Self {
        debug_assert_eq!(
            self.party, rhs.party,
            "cannot add additive shares held by different parties"
        );
        debug_assert_eq!(self.is_leader, rhs.is_leader);
        Self {
            value: self.value + &rhs.value,
            party: self.party,
            is_leader: self.is_leader,
        }
    }
}

impl<T: Ring> Add<&T> for AdditiveSS<T> {
    type Output = Self;

    /// Adds a public constant: `[x] + c = [x + c]`. Only the leader absorbs `c` into its share, so
    /// the shares still sum to `x + c`; every other party leaves its share unchanged.
    fn add(self, rhs: &T) -> Self {
        let value = if self.is_leader {
            self.value + rhs
        } else {
            self.value
        };
        Self {
            value,
            party: self.party,
            is_leader: self.is_leader,
        }
    }
}

impl<T: Ring> Sub<&Self> for AdditiveSS<T> {
    type Output = Self;

    /// Subtracts two shares held by the same party: `[x] - [y] = [x - y]`.
    fn sub(self, rhs: &Self) -> Self {
        debug_assert_eq!(
            self.party, rhs.party,
            "cannot subtract additive shares held by different parties"
        );
        debug_assert_eq!(self.is_leader, rhs.is_leader);
        Self {
            value: self.value - &rhs.value,
            party: self.party,
            is_leader: self.is_leader,
        }
    }
}

impl<T: Ring> Sub<&T> for AdditiveSS<T> {
    type Output = Self;

    /// Subtracts a public constant: `[x] - c = [x - c]`. Only the leader adjusts its share.
    fn sub(self, rhs: &T) -> Self {
        let value = if self.is_leader {
            self.value - rhs
        } else {
            self.value
        };
        Self {
            value,
            party: self.party,
            is_leader: self.is_leader,
        }
    }
}

impl<T: Ring> Mul<&T> for AdditiveSS<T> {
    type Output = Self;

    /// Multiplies by a public scalar: `c · [x] = [c · x]`. Every party scales its share, since
    /// `c · x = c · (x_1 + ... + x_n) = c·x_1 + ... + c·x_n`.
    fn mul(self, rhs: &T) -> Self {
        Self {
            value: self.value * rhs,
            party: self.party,
            is_leader: self.is_leader,
        }
    }
}

impl<T: Ring> Neg for AdditiveSS<T> {
    type Output = Self;

    /// Negates a share: `-[x] = [-x]`.
    fn neg(self) -> Self {
        Self {
            value: self.value.negate(),
            party: self.party,
            is_leader: self.is_leader,
        }
    }
}

impl<T: Ring> LinearShare for AdditiveSS<T>
where
    T: Send + Sync,
{
    type Value = T;

    /// Additive sharing has no threshold to choose: reconstruction structurally requires **all**
    /// shares, so the dealing parameter is `()`.
    type Threshold = ();

    /// Additive sharing does not place parties in the field, so this is never consulted and simply
    /// returns the zero element.
    fn encode_party(_party: PartyId) -> T {
        T::ZERO
    }

    fn secret_from_shares(shares: &[Self], parties: &[PartyId]) -> Result<T, ShareError<T>> {
        if shares.len() != parties.len() {
            return Err(ShareError::LengthMismatch {
                parties_idx_len: parties.len(),
                shares_len: shares.len(),
            });
        }
        // Resolves to the inherent `secret_from_shares(&[Self])` (single argument).
        Ok(Self::secret_from_shares(shares))
    }

    /// Deals `secret` over `parties`. The threshold is structural — every share is required to
    /// reconstruct — so the `threshold` parameter is `()` and this method cannot fail.
    fn shares_from_secret<R: CryptoRng>(
        secret: T,
        parties: &[PartyId],
        _threshold: (),
        rng: &mut R,
    ) -> Result<Vec<Self>, ShareError<T>> {
        // Resolves to the inherent `shares_from_secret(T, &[PartyId], _)` (three arguments).
        Ok(Self::shares_from_secret(secret, parties, rng))
    }
}
