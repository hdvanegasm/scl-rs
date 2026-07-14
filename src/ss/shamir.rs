use std::ops::{Add, Mul, Neg, Sub};

use crate::{
    abbreviate::Abbreviate,
    math::{
        field::FiniteField,
        poly::{interpolate_polynomial_at, Polynomial},
        ring::Ring,
    },
    net::PartyId,
};
use rand::CryptoRng;
use serde::{Deserialize, Serialize};

use super::{LinearShare, ShareError};

/// Represents a Shamir secret share computed with a polynomial of degree `degree`.
#[derive(Serialize, Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ShamirSS<const LIMBS: usize, F> {
    /// Value of the share in the field. If the shares are computed considering a polynomial `p`,
    /// then, this is the value of `p(i)` for the party `i`-th.
    share: F,
    /// The degree of the polynomial used to compute this share.
    degree: usize,
}

impl<const LIMBS: usize, F> Abbreviate for ShamirSS<LIMBS, F> {
    const ABBREVIATION: &'static str = "Shamir shr.";
}

impl<const LIMBS: usize, F> ShamirSS<LIMBS, F>
where
    F: FiniteField<LIMBS>,
{
    /// Creates a new Shamir secret share.
    ///
    /// This is the low-level constructor from an explicit share value and its polynomial degree.
    /// Most callers instead deal shares from a secret with
    /// [`shares_from_secret`](ShamirSS::shares_from_secret).
    ///
    /// # Examples
    ///
    /// ```
    /// use scl_rs::math::field::mersenne61::Mersenne61;
    /// use scl_rs::ss::shamir::ShamirSS;
    ///
    /// let share = ShamirSS::<1, Mersenne61>::new(Mersenne61::from(42u64), 3);
    /// assert_eq!(*share.share(), Mersenne61::from(42u64));
    /// assert_eq!(share.degree(), 3);
    /// ```
    pub fn new(share: F, degree: usize) -> Self {
        Self { share, degree }
    }

    /// Computes a share from a secret.
    ///
    /// The sharing polynomial hides the secret in its other coefficients, so `rng` is bound on
    /// [`CryptoRng`] to keep callers from sampling those coefficients with a predictable
    /// (non-cryptographic) generator. Pass a cryptographically secure source such as `rand::rng()`
    /// or a `ChaCha20Rng` seeded from OS entropy.
    ///
    /// # Examples
    ///
    /// ```
    /// use scl_rs::math::field::mersenne61::Mersenne61;
    /// use scl_rs::ss::shamir::ShamirSS;
    ///
    /// let mut rng = rand::rng();
    /// let secret = Mersenne61::from(1234u64);
    /// let degree = 2;
    /// let indexes: Vec<Mersenne61> = (1..=5u64).map(Mersenne61::from).collect();
    ///
    /// let (shares, _polynomial) = ShamirSS::shares_from_secret(secret, degree, &indexes, &mut rng);
    ///
    /// // Any `degree + 1` shares reconstruct the secret.
    /// let recovered =
    ///     ShamirSS::secret_from_shares(&shares[..degree + 1], &indexes[..degree + 1]).unwrap();
    /// assert_eq!(recovered, secret);
    /// ```
    pub fn shares_from_secret(
        secret: F,
        degree: usize,
        party_indexes: &[F],
        rng: &mut impl CryptoRng,
    ) -> (Vec<Self>, Polynomial<F>) {
        let mut polynomial = Polynomial::random(degree, rng);
        polynomial.set_constant_coeff(secret);
        let shares = party_indexes
            .iter()
            .map(|idx| Self::new(polynomial.evaluate(idx), degree))
            .collect();
        (shares, polynomial)
    }

    /// Returns the value of the share.
    pub fn share(&self) -> &F {
        &self.share
    }

    /// Returns the degree of the polynomial used to compute this share.
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// Retrieves the secret from a set of shares considering an encoding of the party indexes in
    /// the field `F`.
    ///
    /// This protocol reconstructs a secret from its shares by computing Lagrange interpolation.
    ///
    /// # Errors
    ///
    /// If the list of `shares` do not match the length of `party_indexes`, this function will
    /// return a [`ShareError::EvalAndShareLenMismatch`] error.
    pub fn secret_from_shares(shares: &[Self], party_indexes: &[F]) -> Result<F, ShareError<F>> {
        if shares.is_empty() {
            return Err(ShareError::NotEnoughShares);
        }

        if shares.len() != party_indexes.len() {
            return Err(ShareError::EvalAndShareLenMismatch {
                n_eval_points: shares.len(),
                n_shares: party_indexes.len(),
            });
        }

        let deg_first_shr = shares[0].degree();
        if !shares.iter().all(|share| share.degree() == deg_first_shr) {
            return Err(ShareError::SharesWithDifferentDegree);
        }

        if party_indexes.len() < deg_first_shr + 1 || shares.len() < deg_first_shr + 1 {
            return Err(ShareError::NotEnoughShares);
        }

        let evaluations: Vec<F> = shares.iter().map(|share| *share.share()).collect();

        let secret = interpolate_polynomial_at(&evaluations, party_indexes, &F::ZERO)
            .map_err(ShareError::ReconstructionError)?;
        Ok(secret)
    }
}

// --- Local (communication-free) linear operations. See [`LinearShare`] for their MPC meaning. ---

impl<const LIMBS: usize, F: Ring> Add<&Self> for ShamirSS<LIMBS, F> {
    type Output = Self;

    /// Adds two shares: `[x] + [y] = [x + y]`. Both must come from polynomials of the same degree.
    fn add(self, rhs: &Self) -> Self {
        debug_assert_eq!(
            self.degree, rhs.degree,
            "cannot add Shamir shares of different degree"
        );
        Self {
            share: self.share + &rhs.share,
            degree: self.degree,
        }
    }
}

impl<const LIMBS: usize, F: Ring> Add<&F> for ShamirSS<LIMBS, F> {
    type Output = Self;

    /// Adds a public constant: `[x] + c = [x + c]`. Every party adds `c` to its share, which shifts
    /// the sharing polynomial's constant term by `c` and leaves the degree unchanged.
    fn add(self, rhs: &F) -> Self {
        Self {
            share: self.share + rhs,
            degree: self.degree,
        }
    }
}

impl<const LIMBS: usize, F: Ring> Sub<&Self> for ShamirSS<LIMBS, F> {
    type Output = Self;

    /// Subtracts two shares: `[x] - [y] = [x - y]`. Both must have the same degree.
    fn sub(self, rhs: &Self) -> Self {
        debug_assert_eq!(
            self.degree, rhs.degree,
            "cannot subtract Shamir shares of different degree"
        );
        Self {
            share: self.share - &rhs.share,
            degree: self.degree,
        }
    }
}

impl<const LIMBS: usize, F: Ring> Sub<&F> for ShamirSS<LIMBS, F> {
    type Output = Self;

    /// Subtracts a public constant: `[x] - c = [x - c]`.
    fn sub(self, rhs: &F) -> Self {
        Self {
            share: self.share - rhs,
            degree: self.degree,
        }
    }
}

impl<const LIMBS: usize, F: Ring> Mul<&F> for ShamirSS<LIMBS, F> {
    type Output = Self;

    /// Multiplies by a public scalar: `c · [x] = [c · x]`. Scales the whole polynomial, so the
    /// degree is unchanged (unlike multiplying two shares, which is not a linear operation).
    fn mul(self, rhs: &F) -> Self {
        Self {
            share: self.share * rhs,
            degree: self.degree,
        }
    }
}

impl<const LIMBS: usize, F: Ring> Mul<&Self> for ShamirSS<LIMBS, F> {
    type Output = Self;

    fn mul(self, rhs: &Self) -> Self {
        Self {
            share: self.share * &rhs.share,
            degree: self.degree + rhs.degree,
        }
    }
}

impl<const LIMBS: usize, F: Ring> Neg for ShamirSS<LIMBS, F> {
    type Output = Self;

    /// Negates a share: `-[x] = [-x]`.
    fn neg(self) -> Self {
        Self {
            share: self.share.negate(),
            degree: self.degree,
        }
    }
}

impl<const LIMBS: usize, F> LinearShare for ShamirSS<LIMBS, F>
where
    F: FiniteField<LIMBS> + From<u64> + Send + Sync,
{
    type Value = F;

    /// The **degree** of the sharing polynomial: any `degree + 1` shares reconstruct the secret.
    type Threshold = usize;

    /// Places party `i` at the field point `i + 1`, using the field's `u64` conversion. The shift
    /// keeps the mapping injective (for party ids below the field modulus) while never touching
    /// `F::ZERO` — the secret's own evaluation point — so the usual `0`-based network party ids
    /// are safe.
    fn encode_party(party: PartyId) -> F {
        F::from(party.as_usize() as u64 + 1)
    }

    fn secret_from_shares(shares: &[Self], parties: &[PartyId]) -> Result<F, ShareError<F>> {
        let indexes: Vec<F> = parties.iter().copied().map(Self::encode_party).collect();
        // Resolves to the inherent `secret_from_shares(&[Self], &[F])` (different signature).
        Self::secret_from_shares(shares, &indexes)
    }

    /// Deals `secret` with a caller-chosen polynomial degree (the threshold): any `degree + 1` of
    /// the returned shares reconstruct the secret, so lower degrees tolerate more absent parties.
    ///
    /// # Errors
    ///
    /// Returns [`ShareError::InvalidThreshold`] if `degree >= parties.len()`: such a sharing could
    /// never be reconstructed, even with every dealt share.
    fn shares_from_secret<R: CryptoRng>(
        secret: F,
        parties: &[PartyId],
        degree: usize,
        rng: &mut R,
    ) -> Result<Vec<Self>, ShareError<F>> {
        if degree >= parties.len() {
            return Err(ShareError::InvalidThreshold {
                threshold: degree,
                n_parties: parties.len(),
            });
        }
        let indexes: Vec<F> = parties.iter().copied().map(Self::encode_party).collect();
        // Resolves to the inherent `shares_from_secret(F, usize, &[F], _)` (4 args).
        let (shares, _polynomial) = Self::shares_from_secret(secret, degree, &indexes, rng);
        Ok(shares)
    }
}

/// A **double sharing**: a degree-`t` and a degree-`2t` sharing of one and the same secret.
///
/// Multiplying two degree-`t` shares locally gives a degree-`2t` sharing of the product whose
/// polynomial is not uniformly random, so it cannot simply be opened. A double sharing repairs
/// both problems at once: adding its degree-`2t` half masks the product so it is safe to open, and
/// subtracting its degree-`t` half brings the result back down to degree `t`. This is the
/// correlated randomness that makes multiplication possible; see
/// [`PassiveRandDoubleShr`](crate::protocol::passive_shamir::double_rand_share::PassiveRandDoubleShr)
/// for how it is produced without any party learning the secret.
///
/// Deliberately **not** [`Clone`]: a double sharing is one-shot randomness. Reusing one across two
/// multiplications would mask both products with the same value and leak them, so it is consumed by
/// [`into_parts`](DoubleShare::into_parts).
pub struct DoubleShare<const LIMBS: usize, F> {
    share_t: ShamirSS<LIMBS, F>,
    share_2t: ShamirSS<LIMBS, F>,
}

impl<const LIMBS: usize, F> DoubleShare<LIMBS, F>
where
    F: FiniteField<LIMBS>,
{
    /// Pairs a degree-`t` sharing with a degree-`2t` sharing of the same secret.
    ///
    /// # Panics
    ///
    /// Panics unless `share_2t`'s degree is exactly twice `share_t`'s. The caller is responsible
    /// for the two halves hiding the same secret, which the degrees alone cannot witness.
    pub fn new(share_t: ShamirSS<LIMBS, F>, share_2t: ShamirSS<LIMBS, F>) -> Self {
        assert_eq!(2 * share_t.degree, share_2t.degree);
        Self { share_t, share_2t }
    }

    /// Returns `t`: the degree of the lower half, half the degree of the upper one.
    pub fn degree(&self) -> usize {
        self.share_t.degree
    }

    /// Consumes the double sharing, returning the degree-`t` and degree-`2t` halves in that order.
    pub fn into_parts(self) -> (ShamirSS<LIMBS, F>, ShamirSS<LIMBS, F>) {
        (self.share_t, self.share_2t)
    }

    /// Borrows the degree-`t` and degree-`2t` halves, in that order.
    pub fn parts(&self) -> (&ShamirSS<LIMBS, F>, &ShamirSS<LIMBS, F>) {
        (&self.share_t, &self.share_2t)
    }
}
