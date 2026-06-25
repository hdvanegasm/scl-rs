use crate::{
    abbreviate::Abbreviate,
    math::{
        field::FiniteField,
        poly::{interpolate_polynomial_at, Polynomial},
    },
};
use rand::CryptoRng;
use serde::{Deserialize, Serialize};

use super::ShareError;

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
    pub fn new(share: F, degree: usize) -> Self {
        Self { share, degree }
    }

    /// Computes a share from a secret.
    ///
    /// The sharing polynomial hides the secret in its other coefficients, so `rng` is bound on
    /// [`CryptoRng`] to keep callers from sampling those coefficients with a predictable
    /// (non-cryptographic) generator. Pass a cryptographically secure source such as `rand::rng()`
    /// or a `ChaCha20Rng` seeded from OS entropy.
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
