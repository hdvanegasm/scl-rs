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
    ///
    /// This is the low-level constructor pairing a Shamir share with the dealer's commitment
    /// vector. Most callers instead deal shares from a secret with
    /// [`shares_from_secret`](FeldmanSS::shares_from_secret), which builds the commitments for them.
    ///
    /// # Examples
    ///
    /// ```
    /// use scl_rs::math::ec::secp256k1::Secp256k1;
    /// use scl_rs::math::field::secp256k1_scalar::Secp256k1ScalarField;
    /// use scl_rs::prelude::{EllipticCurve, Ring};
    /// use scl_rs::ss::{feldman::FeldmanSS, shamir::ShamirSS};
    ///
    /// let share = ShamirSS::<4, Secp256k1ScalarField>::new(Secp256k1ScalarField::ONE, 1);
    /// let commitments = vec![Secp256k1::gen(), Secp256k1::gen()];
    /// let feldman = FeldmanSS::new(share, commitments);
    ///
    /// assert_eq!(feldman.shamir_share().degree(), 1);
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use scl_rs::math::ec::secp256k1::Secp256k1;
    /// use scl_rs::math::field::secp256k1_scalar::Secp256k1ScalarField;
    /// use scl_rs::prelude::Ring;
    /// use scl_rs::ss::feldman::FeldmanSS;
    ///
    /// let mut rng = rand::rng();
    /// let secret = Secp256k1ScalarField::random(&mut rng);
    /// let degree = 2;
    /// let indexes: Vec<Secp256k1ScalarField> =
    ///     (1..=5u64).map(Secp256k1ScalarField::from).collect();
    ///
    /// let shares: Vec<FeldmanSS<4, Secp256k1>> =
    ///     FeldmanSS::shares_from_secret(secret, degree, &indexes, &mut rng);
    ///
    /// // Every share verifies against the dealer's published commitments...
    /// for (share, idx) in shares.iter().zip(&indexes) {
    ///     assert!(share.is_valid(*idx));
    /// }
    /// // ...and any `degree + 1` of them reconstruct the secret.
    /// let recovered =
    ///     FeldmanSS::secret_from_shares(&shares[..degree + 1], &indexes[..degree + 1]).unwrap();
    /// assert_eq!(recovered, secret);
    /// ```
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

/// Unit tests for Feldman share verification and for the errors reconstruction reports.
#[cfg(test)]
mod tests {
    use super::{FeldmanSS, ShamirSS, ShareError};
    use crate::math::{
        ec::secp256k1::Secp256k1,
        field::{secp256k1_prime::Secp256k1PrimeField, secp256k1_scalar::Secp256k1ScalarField},
        ring::Ring,
    };

    /// Degree of the dealt sharing polynomial.
    const T: usize = 3;
    /// Number of parties dealt a share.
    const N: usize = 7;

    /// Returns the party indexes `1, …, n` as scalar-field elements.
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

    /// Checks that every share of an honest dealing verifies against its commitments.
    #[test]
    fn honest_shares_are_valid() {
        let (shares, indexes) = honest_shares();
        for (share, idx) in shares.iter().zip(&indexes) {
            assert!(share.is_valid(*idx));
        }
    }

    /// Checks that reconstruction from `T + 1` shares but only `T` indexes fails with
    /// [`ShareError::LengthMismatch`] carrying both lengths.
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

    /// Checks that a share whose value was changed after dealing fails verification, and that
    /// reconstruction including it fails with [`ShareError::InvalidShare`] naming that party.
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

    /// Checks that a share whose first commitment is replaced by a point not on the curve fails
    /// verification, and that reconstruction including it fails with [`ShareError::InvalidShare`]
    /// naming that party.
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

    /// Checks that a share with fewer than `degree + 1` commitments fails verification.
    #[test]
    fn wrong_commitment_vector_length_is_rejected() {
        let (mut shares, indexes) = honest_shares();
        // Drop a commitment so the vector length no longer equals `degree + 1`.
        shares[0].commitments.pop();
        assert!(!shares[0].is_valid(indexes[0]));
    }
}
