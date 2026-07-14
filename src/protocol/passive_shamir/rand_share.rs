use crate::{
    math::{field::FiniteField, matrix::Matrix, vector::Vector},
    net::{Network, PartyId},
    protocol::{
        passive_shamir::extraction_matrix, share::deal::PassiveDealShr, Error, Protocol,
        ProtocolId, RandEnvironment,
    },
    ss::shamir::ShamirSS,
};

/// DN07 `Random`: produces a batch of degree-`t` sharings of secrets that **no party knows**.
///
/// Every party deals one degree-`t` sharing of a value it samples itself, and the `n` dealt
/// sharings are compressed with a Vandermonde extraction matrix into `n - t` sharings that are
/// uniformly random even if up to `t` of the dealers colluded to pick their inputs. One run
/// therefore yields `n - t` shares for the cost of one dealing round.
///
/// `parties` is held sorted, so column `i` of the extraction matrix refers to the same dealer at
/// every party — the shares must be combined in the same order everywhere or the outputs do not lie
/// on a common polynomial.
pub struct PassiveRandShr<const LIMBS: usize, F> {
    t: usize,
    parties: Vec<PartyId>,
    vandermonde: Matrix<F>,
}

impl<const LIMBS: usize, F> PassiveRandShr<LIMBS, F>
where
    F: FiniteField<LIMBS> + From<u64> + Send + Sync,
{
    /// Creates the protocol for a run among `parties` tolerating up to `t` passive corruptions.
    ///
    /// Every party in `parties` must run this protocol, and each run outputs `parties.len() - t`
    /// shares of degree `t`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Input`] if `t >= parties.len()`: a degree-`t` sharing could not be
    /// reconstructed even from every dealt share, and there would be no randomness left to extract.
    pub fn new(t: usize, mut parties: Vec<PartyId>) -> Result<Self, Error> {
        let n = parties.len();
        if t >= n {
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

#[async_trait::async_trait]
impl<const LIMBS: usize, E, F> Protocol<E> for PassiveRandShr<LIMBS, F>
where
    F: FiniteField<LIMBS> + Send + Sync + From<u64> + 'static,
    E: RandEnvironment,
{
    type Output = Vec<ShamirSS<LIMBS, F>>;

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
                let other_share_t = pkt.pop()?;
                shares_t.push(other_share_t);
            } else {
                shares_t.push(own_share_t.clone());
            }
        }

        // Multiply by Vandermonde matrix.
        let v_shares_t = Vector::from(shares_t);

        let r_shares_t = self
            .vandermonde
            .mul_shares(&v_shares_t)
            .map_err(|e| Error::Share(Box::new(e)))?
            .into_iter()
            .collect();

        Ok(r_shares_t)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PassiveRandShr")
    }
}
