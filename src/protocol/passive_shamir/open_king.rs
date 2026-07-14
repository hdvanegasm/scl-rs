use crate::{
    abbreviate::Abbreviate,
    math::field::FiniteField,
    net::{Network, Packet, PartyId},
    protocol::{Error, Protocol, ProtocolId, RandEnvironment},
    ss::{shamir::ShamirSS, LinearShare},
};

/// DN07 `Open`: reconstructs one shared secret and gives it to every party.
///
/// Each party sends its share to a designated **king**, who interpolates the secret from all `n`
/// shares and sends it back to everyone. Routing through a king costs two rounds but only `O(n)`
/// messages, where having every party broadcast to every other would cost `O(n²)`.
///
/// Opening many secrets one at a time costs two rounds *each*; use
/// [`BatchedPassiveOpenToKing`] to open a whole batch in the same two rounds.
///
/// # Security model: passive adversary
///
/// The king is trusted to interpolate honestly and report the same value to everyone — safe only
/// against a semi-honest adversary. Note also that the king sees all `n` shares, so it learns the
/// whole sharing polynomial, not just the secret; the callers here only ever open values that are
/// masked or otherwise safe to reveal.
pub struct PassiveOpenToKing<const LIMBS: usize, F> {
    king: PartyId,
    parties: Vec<PartyId>,
    own_share: ShamirSS<LIMBS, F>,
}

impl<const LIMBS: usize, F> PassiveOpenToKing<LIMBS, F> {
    /// Creates the protocol for the local party, which contributes `own_share`.
    ///
    /// Every party in `parties` must run this protocol, and all of them must pass the same `king`
    /// (itself one of `parties`) and the same party list.
    pub fn new(king: PartyId, parties: Vec<PartyId>, own_share: ShamirSS<LIMBS, F>) -> Self {
        Self {
            king,
            parties,
            own_share,
        }
    }
}

#[async_trait::async_trait]
impl<const LIMBS: usize, E, F> Protocol<E> for PassiveOpenToKing<LIMBS, F>
where
    F: FiniteField<LIMBS> + Send + Sync + From<u64> + Abbreviate + 'static,
    E: RandEnvironment,
{
    type Output = F;

    async fn run(mut self, environment: &mut E) -> Result<Self::Output, Error> {
        let me = environment.network().local_party();
        if me == self.king {
            let mut shares = Vec::new();
            for party in &self.parties {
                if *party != me {
                    let mut pkt = environment.network_mut().recv_from(*party).await?;
                    let share = pkt.pop()?;
                    shares.push(share);
                } else {
                    shares.push(self.own_share.clone());
                }
            }
            let secret =
                <ShamirSS<LIMBS, F> as LinearShare>::secret_from_shares(&shares, &self.parties)?;
            for party in self.parties.iter().filter(|&&party| party != self.king) {
                let mut pkt = Packet::empty();
                pkt.write_labeled(&secret)?;
                environment.network_mut().send_to(*party, &pkt).await?;
            }
            Ok(secret)
        } else {
            let mut pkt = Packet::empty();
            pkt.write_labeled(&self.own_share)?;
            environment.network_mut().send_to(self.king, &pkt).await?;

            let mut pkt = environment.network_mut().recv_from(self.king).await?;
            let secret = pkt.pop()?;
            Ok(secret)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PassiveOpenToKing")
    }
}

/// The batched form of [`PassiveOpenToKing`]: opens a whole vector of secrets in **two rounds**,
/// no matter how many there are.
///
/// Each party sends all of its shares to the king in a single packet; the king reconstructs every
/// secret and returns them all in a single packet. The round count is therefore independent of the
/// batch size, which is what keeps protocols built on it — triple generation above all — from
/// paying a round trip per value.
pub struct BatchedPassiveOpenToKing<const LIMBS: usize, F> {
    king: PartyId,
    parties: Vec<PartyId>,
    own_shares: Vec<ShamirSS<LIMBS, F>>,
}

impl<const LIMBS: usize, F> BatchedPassiveOpenToKing<LIMBS, F> {
    /// Creates the protocol for the local party, which contributes one share per secret to open.
    ///
    /// The `i`-th output is reconstructed from the `i`-th entry of every party's `own_shares`, so
    /// all parties must pass their shares in the same order, and every party must pass the same
    /// number of them, the same `king` (itself one of `parties`) and the same party list.
    pub fn new(king: PartyId, parties: Vec<PartyId>, own_shares: Vec<ShamirSS<LIMBS, F>>) -> Self {
        Self {
            king,
            parties,
            own_shares,
        }
    }
}

#[async_trait::async_trait]
impl<const LIMBS: usize, E, F> Protocol<E> for BatchedPassiveOpenToKing<LIMBS, F>
where
    F: FiniteField<LIMBS> + Send + Sync + From<u64> + Abbreviate + 'static,
    E: RandEnvironment,
{
    type Output = Vec<F>;

    async fn run(mut self, environment: &mut E) -> Result<Self::Output, Error> {
        let me = environment.network().local_party();
        if me == self.king {
            let n_shares = self.own_shares.len();
            let mut shares_per_party = Vec::new();
            for party in &self.parties {
                if *party != me {
                    let pkt = environment.network_mut().recv_from(*party).await?;
                    let mut shares = Vec::with_capacity(n_shares);
                    for i in 0..n_shares {
                        shares.push(pkt.read(i)?);
                    }

                    shares_per_party.push(shares);
                } else {
                    shares_per_party.push(self.own_shares.clone());
                }
            }
            let mut secrets = Vec::new();
            for i in 0..n_shares {
                let mut shares = Vec::new();
                for share_list in &shares_per_party {
                    shares.push(share_list[i].clone());
                }

                let secret = <ShamirSS<LIMBS, F> as LinearShare>::secret_from_shares(
                    &shares,
                    &self.parties,
                )?;
                secrets.push(secret);
            }

            for party in self.parties.iter().filter(|&&party| party != self.king) {
                let mut pkt = Packet::empty();
                pkt.write_many_labeled(&secrets)?;
                environment.network_mut().send_to(*party, &pkt).await?;
            }

            Ok(secrets)
        } else {
            let n_shares = self.own_shares.len();

            let mut pkt = Packet::empty();
            pkt.write_many_labeled(&self.own_shares)?;
            environment.network_mut().send_to(self.king, &pkt).await?;

            let pkt = environment.network_mut().recv_from(self.king).await?;
            let mut secrets = Vec::with_capacity(n_shares);
            for i in 0..n_shares {
                secrets.push(pkt.read(i)?);
            }
            Ok(secrets)
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("BatchedPassiveOpenToKing")
    }
}
