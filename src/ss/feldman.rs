use super::{shamir::ShamirSS, ShareError};
use crate::{
    abbreviate::Abbreviate,
    math::{ec::EllipticCurve, ring::Ring},
};
use rand::CryptoRng;
use serde::{Deserialize, Serialize};

/// Represents a Feldman Secret Sharing element.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
#[serde(bound(
    serialize = "C: Serialize, C::ScalarField: Serialize",
    deserialize = "C: Serialize, C::ScalarField: Serialize"
))]
pub struct FeldmanSS<const LIMBS: usize, C: EllipticCurve<LIMBS>> {
    /// The Shamir secret sharing for the Feldman representation.
    shamir_share: ShamirSS<LIMBS, C::ScalarField>,
    /// The commitment of this share.
    commitments: Vec<C>,
}

impl<const LIMBS: usize, C: EllipticCurve<LIMBS>> Abbreviate for FeldmanSS<LIMBS, C> {
    const ABBREVIATION: &'static str = "Feldman shr.";
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
    pub fn is_valid(&self, owner: C::ScalarField) -> bool {
        if self.commitments.len() != self.shamir_share.degree() + 1 {
            return false;
        }

        // We check that g^{s_i} = g^{p(i)} = g^{a_0 + a_1 * i + ... + a_{t} * i^{t}}.
        let mut inner_prod = C::ZERO;
        for (exp, commitment) in self.commitments.iter().enumerate() {
            inner_prod = inner_prod.add(&commitment.scalar_mul(&owner.pow(exp as u64)));
        }

        inner_prod == C::gen().scalar_mul(self.shamir_share().share())
    }

    /// Returns the Shamir secret share associated with the Feldman share.
    pub fn shamir_share(&self) -> &ShamirSS<LIMBS, C::ScalarField> {
        &self.shamir_share
    }

    /// Computes the Feldman Shares of a secret element.
    ///
    /// The underlying Shamir sharing hides the secret in the polynomial coefficients it samples, so
    /// `rng` is bound on [`CryptoRng`] to keep callers from generating secret material with a
    /// predictable (non-cryptographic) generator. Pass a cryptographically secure source such as
    /// `rand::rng()` or a `ChaCha20Rng` seeded from OS entropy.
    pub fn shares_from_secret(
        secret: C::ScalarField,
        degree: usize,
        party_indexes: &[C::ScalarField],
        rng: &mut impl CryptoRng,
    ) -> Vec<Self> {
        let (shamir_shares, polynomial) =
            ShamirSS::shares_from_secret(secret, degree, party_indexes, rng);
        let mut commitments = Vec::with_capacity(polynomial.degree() + 1);
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
        if shares.len() != party_indexes.len() {
            return Err(ShareError::LengthMismatch {
                parties_idx_len: party_indexes.len(),
                shares_len: shares.len(),
            });
        }
        for (share, party_idx) in shares.iter().zip(party_indexes) {
            if !share.is_valid(*party_idx) {
                return Err(ShareError::InvalidShare {
                    party_idx: *party_idx,
                });
            }
        }
        let shamir_shares: Vec<ShamirSS<LIMBS, C::ScalarField>> = shares
            .iter()
            .map(|share| share.shamir_share().clone())
            .collect();
        let secret = ShamirSS::secret_from_shares(&shamir_shares, party_indexes)?;
        Ok(secret)
    }
}
