use crate::net::{Network, Packet, PartyId};
use crate::prelude::{Abbreviate, Environment, Error, Protocol};
use crate::protocol::ProtocolId;
use crate::ss::LinearShare;

/// Protocol in which a designated **dealer** splits a secret into shares of a linear secret
/// sharing scheme `S` and distributes one share to each receiver.
///
/// All parties run the same protocol, but only the dealer knows the secret: construct the
/// dealer's instance with [`dealer`](PassiveDealShr::dealer) and every other party's with
/// [`receiver`](PassiveDealShr::receiver). The dealer computes the shares with
/// [`LinearShare::shares_from_secret`] and scatters them (one round); every party — the dealer
/// included — then receives its own share from the dealer and returns it as the protocol output.
///
/// # Preconditions
///
/// - The **dealer must be listed among its own receivers**: every party running the protocol ends
///   by waiting for its share from the dealer, so a dealer that excludes itself blocks forever.
/// - Only the parties in `receivers` may run the protocol: a party outside that list never gets a
///   share and blocks waiting for it.
///
/// Note that dealing draws its randomness from the scheme's trait-level
/// [`LinearShare::shares_from_secret`], which is not seed-reproducible.
///
/// # Security model: passive adversary
///
/// This protocol assumes a **passive (semi-honest) adversary**: every party follows the protocol,
/// so in particular the dealer always distributes well-formed shares to every receiver, and each
/// receiver can safely block waiting for its share. A dealer that crashes, withholds a share, or
/// deals inconsistently is outside this model; lifting the assumption (receive timeouts,
/// verifiable dealing) is planned malicious-model work — see `docs/roadmap.md` §11.
pub struct PassiveDealShr<S>
where
    S: LinearShare,
{
    /// The party that knows the secret and distributes its shares.
    dealer: PartyId,
    /// The dealer-only input; `None` on receivers.
    secret_info: Option<DealerInfo<S::Value>>,
}

/// The input that only the dealer holds: the secret and whom to hand shares to.
struct DealerInfo<T> {
    /// The secret to split into shares.
    secret: T,
    /// The parties that receive a share, one each; `receivers[i]` gets the `i`-th share.
    receivers: Vec<PartyId>,
}

impl<S> PassiveDealShr<S>
where
    S: LinearShare,
{
    /// Creates the protocol instance for the **dealer**: the party (`dealer`, which must be the
    /// local party) that splits `secret` into one share per party in `receivers` and distributes
    /// them. The dealer must include itself in `receivers` to obtain its own share (see the
    /// preconditions on [`PassiveDealShr`]).
    pub fn dealer(dealer: PartyId, secret: S::Value, receivers: Vec<PartyId>) -> Self {
        Self {
            dealer,
            secret_info: Some(DealerInfo { secret, receivers }),
        }
    }

    /// Creates the protocol instance for a **receiver**: a party that provides no secret and only
    /// waits for its share from `dealer`.
    pub fn receiver(dealer: PartyId) -> Self {
        Self {
            dealer,
            secret_info: None,
        }
    }
}

#[async_trait::async_trait]
impl<S, E> Protocol<E> for PassiveDealShr<S>
where
    S: LinearShare + Abbreviate,
    E: Environment,
    S::Value: Sync + Send,
{
    type Output = S;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let me = env.network().local_party();

        // Compute and send the shares if I am the dealer.
        if self.dealer == me {
            let dealer_info = self.secret_info.ok_or(Error::Input)?;
            let shares = S::shares_from_secret(dealer_info.secret, &dealer_info.receivers);
            let mut messages = Vec::with_capacity(shares.len());
            for (recv, share) in dealer_info.receivers.into_iter().zip(shares) {
                let mut pkt = Packet::empty();
                pkt.write_labeled(&share)?;
                messages.push((recv, pkt));
            }
            env.network_mut().send_many(&messages).await?;
        }

        // Receive your share.
        let own_shr = env.network_mut().recv_from(self.dealer).await?.pop()?;
        Ok(own_shr)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PassiveDealLinearShr")
    }
}
