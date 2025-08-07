use super::{shamir::ShamirSS, ShareError};
use crate::math::{ec::EllipticCurve, poly::compute_lagrange_basis};
use crypto_bigint::rand_core::RngCore;
use serde::Serialize;

/// Represents a Feldman Secret Sharing element.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct FeldmanSS<const LIMBS: usize, C: EllipticCurve<LIMBS>> {
    /// The Shamir secret sharing for the Feldman representation.
    shamir_share: ShamirSS<LIMBS, C::ScalarField>,
    /// The commitment of this share.
    commitments: Vec<C>,
}

impl<const LIMBS: usize, C: EllipticCurve<LIMBS>> FeldmanSS<LIMBS, C> {
    /// Creates a new Feldman Secret Sharing element.
    pub fn new(shamir_share: ShamirSS<LIMBS, C::ScalarField>, commitments: Vec<C>) -> Self {
        Self {
            shamir_share,
            commitments,
        }
    }

    /// Checks if the share is valid with respect to the commitment.
    pub fn is_valid(&self, party_indexes: &[C::ScalarField], share_idx: &C::ScalarField) -> bool {
        let lagrange_basis_result = compute_lagrange_basis(party_indexes, share_idx);
        if lagrange_basis_result.is_err() {
            false
        } else {
            let lagrange_basis = lagrange_basis_result.unwrap();
            let mut inner_prod = C::ZERO;
            for (basis, commitment) in lagrange_basis.iter().zip(self.commitments.iter()) {
                inner_prod = inner_prod.add(&commitment.scalar_mul(basis));
            }
            inner_prod == C::gen().scalar_mul(self.shamir_share().share())
        }
    }

    /// Returns the Shamir secret share associated with the Feldman share.
    pub fn shamir_share(&self) -> &ShamirSS<LIMBS, C::ScalarField> {
        &self.shamir_share
    }

    /// Computes the Feldman Shares of a secret element.
    pub fn shares_from_secret(
        secret: C::ScalarField,
        degree: usize,
        party_indexes: &[C::ScalarField],
        rng: &mut impl RngCore,
    ) -> Vec<Self> {
        let (shamir_shares, polynomial) =
            ShamirSS::shares_from_secret(secret, degree, party_indexes, rng);
        let mut commitments = Vec::with_capacity(polynomial.degree());
        for coeff in polynomial.coefficients() {
            commitments.push(C::gen().scalar_mul(coeff));
        }
        shamir_shares
            .into_iter()
            .map(|ss| Self::new(ss, commitments.clone()))
            .collect()
    }

    /// Recovers the secret from its shares.
    pub fn secret_from_shares(
        shares: &[Self],
        party_indexes: &[C::ScalarField],
    ) -> Result<C::ScalarField, ShareError<C::ScalarField>> {
        // Validate shares.
        shares
            .iter()
            .zip(party_indexes)
            .all(|(share, party_idx)| share.is_valid(party_indexes, party_idx));
        let shamir_shares: Vec<ShamirSS<LIMBS, C::ScalarField>> = shares
            .iter()
            .map(|share| share.shamir_share().clone())
            .collect();
        let secret = ShamirSS::secret_from_shares(&shamir_shares, party_indexes)?;
        Ok(secret)
    }
}
