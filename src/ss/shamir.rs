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
    F: FiniteField<LIMBS> + From<u64>,
{
    type Value = F;

    /// Places party `i` at the field point `i`, using the field's `u64` conversion. This is
    /// injective for party ids below the field modulus, and party id `0` maps to `F::ZERO` (the
    /// secret's own point), so party ids must start at `1`.
    fn encode_party(party: PartyId) -> F {
        F::from(party.as_usize() as u64)
    }

    fn secret_from_shares(shares: &[Self], parties: &[PartyId]) -> Result<F, ShareError<F>> {
        let indexes: Vec<F> = parties.iter().copied().map(Self::encode_party).collect();
        // Resolves to the inherent `secret_from_shares(&[Self], &[F])` (different signature).
        Self::secret_from_shares(shares, &indexes)
    }

    /// Deals `secret` as a **full-threshold** (`n`-out-of-`n`) sharing over `parties`: the sharing
    /// polynomial has degree `parties.len() - 1`, so every party's share is required to
    /// reconstruct. For a lower threshold `t < n - 1`, use the inherent
    /// [`ShamirSS::shares_from_secret`], which takes the degree explicitly.
    ///
    /// The trait signature carries no RNG, so the coefficients are drawn from `rand::rng()` (a
    /// CSPRNG). Callers that need a seeded/deterministic RNG must use the inherent method instead.
    fn shares_from_secret(secret: F, parties: &[PartyId]) -> Vec<Self> {
        let indexes: Vec<F> = parties.iter().copied().map(Self::encode_party).collect();
        let degree = parties.len().saturating_sub(1);
        let mut rng = rand::rng();
        // Resolves to the inherent `shares_from_secret(F, usize, &[F], _)` (4 args).
        let (shares, _polynomial) = Self::shares_from_secret(secret, degree, &indexes, &mut rng);
        shares
    }
}
