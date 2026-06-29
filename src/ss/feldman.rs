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

        if !self.commitments.iter().all(|c| c.is_on_curve()) {
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

#[cfg(test)]
mod tests {
    use super::{FeldmanSS, ShamirSS, ShareError};
    use crate::math::{
        ec::secp256k1::Secp256k1,
        field::{secp256k1_prime::Secp256k1PrimeField, secp256k1_scalar::Secp256k1ScalarField},
        ring::Ring,
    };

    const T: usize = 3;
    const N: usize = 7;

    fn party_indexes(n: u64) -> Vec<Secp256k1ScalarField> {
        (1..=n).map(Secp256k1ScalarField::from).collect()
    }

    /// Deals an honest secret and returns the per-party shares together with their indexes.
    fn honest_shares() -> (Vec<FeldmanSS<4, Secp256k1>>, Vec<Secp256k1ScalarField>) {
        let mut rng = rand::rng();
        let secret = Secp256k1ScalarField::random(&mut rng);
        let indexes = party_indexes(N as u64);
        let shares = FeldmanSS::shares_from_secret(secret, T, &indexes, &mut rng);
        (shares, indexes)
    }

    #[test]
    fn honest_shares_are_valid() {
        let (shares, indexes) = honest_shares();
        for (share, idx) in shares.iter().zip(&indexes) {
            assert!(share.is_valid(*idx));
        }
    }

    #[test]
    fn length_mismatch_is_rejected() {
        let (shares, indexes) = honest_shares();
        let err = FeldmanSS::secret_from_shares(&shares[..T + 1], &indexes[..T]).unwrap_err();
        match err {
            ShareError::LengthMismatch {
                parties_idx_len,
                shares_len,
            } => {
                assert_eq!(parties_idx_len, T);
                assert_eq!(shares_len, T + 1);
            }
            other => panic!("expected LengthMismatch, got {other:?}"),
        }
    }

    #[test]
    fn tampered_share_is_detected() {
        let (mut shares, indexes) = honest_shares();
        // Flip party 1's share value while keeping the honest commitments: g^{s'} no longer
        // matches the committed polynomial evaluation, so verification must fail.
        let degree = shares[0].shamir_share().degree();
        let tampered_value = *shares[0].shamir_share().share() + &Secp256k1ScalarField::ONE;
        shares[0].shamir_share = ShamirSS::new(tampered_value, degree);

        assert!(!shares[0].is_valid(indexes[0]));

        let err = FeldmanSS::secret_from_shares(&shares[..T + 1], &indexes[..T + 1]).unwrap_err();
        match err {
            ShareError::InvalidShare { party_idx } => assert_eq!(party_idx, indexes[0]),
            other => panic!("expected InvalidShare, got {other:?}"),
        }
    }

    #[test]
    fn off_curve_commitment_is_rejected() {
        let (mut shares, indexes) = honest_shares();
        // An adversarial dealer supplies a commitment that is not on the curve. The guard must
        // reject it *before* it reaches `scalar_mul`.
        shares[0].commitments[0] = Secp256k1::from_coordinates_unchecked(
            Secp256k1PrimeField::ONE,
            Secp256k1PrimeField::ONE,
            Secp256k1PrimeField::ONE,
        );

        assert!(!shares[0].is_valid(indexes[0]));

        let err = FeldmanSS::secret_from_shares(&shares[..T + 1], &indexes[..T + 1]).unwrap_err();
        match err {
            ShareError::InvalidShare { party_idx } => assert_eq!(party_idx, indexes[0]),
            other => panic!("expected InvalidShare, got {other:?}"),
        }
    }

    #[test]
    fn wrong_commitment_vector_length_is_rejected() {
        let (mut shares, indexes) = honest_shares();
        // Drop a commitment so the vector length no longer equals `degree + 1`.
        shares[0].commitments.pop();
        assert!(!shares[0].is_valid(indexes[0]));
    }
}
