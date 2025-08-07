use crate::math::{
    field::FiniteField,
    poly::{interpolate_polynomial_at, Polynomial},
};
use crypto_bigint::rand_core::RngCore;
use serde::Serialize;

use super::ShareError;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ShamirSS<const LIMBS: usize, F: FiniteField<LIMBS>> {
    share: F,
    degree: usize,
}

impl<const LIMBS: usize, F> ShamirSS<LIMBS, F>
where
    F: FiniteField<LIMBS>,
{
    pub fn new(share: F, degree: usize) -> Self {
        Self { share, degree }
    }

    pub fn shares_from_secret(
        secret: F,
        degree: usize,
        party_indexes: &[F],
        rng: &mut impl RngCore,
    ) -> (Vec<Self>, Polynomial<LIMBS, F>) {
        let mut polynomial = Polynomial::random(degree, rng);
        polynomial.set_constant_coeff(secret);
        let shares = party_indexes
            .iter()
            .map(|idx| Self::new(polynomial.evaluate(idx), degree))
            .collect();
        (shares, polynomial)
    }

    pub fn share(&self) -> &F {
        &self.share
    }

    pub fn degree(&self) -> usize {
        self.degree
    }

    pub fn secret_from_shares(shares: &[Self], party_indexes: &[F]) -> Result<F, ShareError<F>> {
        if shares.is_empty() {
            return Err(ShareError::NotEnoughShares);
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
