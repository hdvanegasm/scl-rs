use crate::{
    math::{field::FiniteField, matrix::Matrix, vector::Vector},
    net::{Network, PartyId},
    protocol::{
        passive_shamir::extraction_matrix, share::deal::PassiveDealShr, Error, Protocol,
        ProtocolId, RandEnvironment,
    },
    ss::shamir::{DoubleShare, ShamirSS},
};

/// DN07 `Double-Random`: produces a batch of [`DoubleShare`]s — pairs of a degree-`t` and a
/// degree-`2t` sharing of the *same* secret, which no party knows.
///
/// Every party deals both a degree-`t` and a degree-`2t` sharing of one value it samples itself,
/// and the same extraction matrix compresses each of the two dealt families into `n - t` sharings.
/// Because the same matrix is applied to both, the `k`-th degree-`t` output and the `k`-th
/// degree-`2t` output share a secret. This is the correlated randomness that re-randomizes a
/// product of two sharings, so it is what makes multiplication possible.
///
/// `parties` is held sorted, so column `i` of the extraction matrix refers to the same dealer at
/// every party.
pub struct PassiveRandDoubleShr<const LIMBS: usize, F> {
    t: usize,
    parties: Vec<PartyId>,
    vandermonde: Matrix<F>,
}

impl<const LIMBS: usize, F> PassiveRandDoubleShr<LIMBS, F>
where
    F: FiniteField<LIMBS> + From<u64> + Send + Sync,
{
    /// Creates the protocol for a run among `parties` tolerating up to `t` passive corruptions.
    ///
    /// Every party in `parties` must run this protocol, and each run outputs `parties.len() - t`
    /// double sharings.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Input`] unless `2 * t < parties.len()`. A degree-`2t` sharing needs
    /// `2t + 1` shares to reconstruct, so with fewer than `2t + 1` parties the degree-`2t` half
    /// could never be opened — this is DN07's `n >= 2t + 1` requirement.
    pub fn new(t: usize, mut parties: Vec<PartyId>) -> Result<Self, Error> {
        let n = parties.len();
        if 2 * t >= n {
            return Err(Error::Input);
        }
        parties.sort();
        let vandermonde = extraction_matrix::<LIMBS, F>(&parties, n - t);
        Ok(Self {
            t,
            parties,
            vandermonde,
        })
    }
}

impl<const LIMBS: usize, E, F> Protocol<E> for PassiveRandDoubleShr<LIMBS, F>
where
    F: FiniteField<LIMBS> + Send + Sync + From<u64> + 'static,
    E: RandEnvironment,
{
    type Output = Vec<DoubleShare<LIMBS, F>>;

    async fn run(self, environment: &mut E) -> Result<Self::Output, Error> {
        // `self.parties` was sorted at construction, which is what aligns each Vandermonde column
        // with the dealer it belongs to, identically at every party.
        let s = F::random(environment.rng_mut());

        let deal_protocol_t = PassiveDealShr::dealer(
            environment.network().local_party(),
            s,
            self.parties.clone(),
            self.t,
        );

        let own_share_t: ShamirSS<LIMBS, F> = deal_protocol_t.execute(environment).await?;
        let mut shares_t = Vec::new();
        for party_id in &self.parties {
            if *party_id != environment.network().local_party() {
                let mut pkt = environment.network_mut().recv_from(*party_id).await?;
                let other_share_t: ShamirSS<LIMBS, F> = pkt.pop()?;
                shares_t.push(other_share_t.clone());
            } else {
                shares_t.push(own_share_t.clone());
            }
        }

        let deal_protocol_2t = PassiveDealShr::dealer(
            environment.network().local_party(),
            s,
            self.parties.clone(),
            2 * self.t,
        );
        let own_share_2t: ShamirSS<LIMBS, F> = deal_protocol_2t.execute(environment).await?;
        let mut shares_2t = Vec::new();
        for party_id in &self.parties {
            if *party_id != environment.network().local_party() {
                let mut pkt = environment.network_mut().recv_from(*party_id).await?;
                let other_share_2t: ShamirSS<LIMBS, F> = pkt.pop()?;
                shares_2t.push(other_share_2t.clone());
            } else {
                shares_2t.push(own_share_2t.clone());
            }
        }

        // Multiply by Vandermonde matrix.
        let v_shares_t = Vector::from(shares_t);
        let v_shares_2t = Vector::from(shares_2t);

        let r_shares_t = self
            .vandermonde
            .mul_shares(&v_shares_t)
            .map_err(|e| Error::Share(Box::new(e)))?;
        let r_shares_2t = self
            .vandermonde
            .mul_shares(&v_shares_2t)
            .map_err(|e| Error::Share(Box::new(e)))?;

        let result = r_shares_t
            .into_iter()
            .zip(r_shares_2t)
            .map(|(r_t, r_2t)| DoubleShare::new(r_t, r_2t))
            .collect();
        Ok(result)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PassiveRandDoubleShr")
    }
}
