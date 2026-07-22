use crate::{
    net::{Network, Packet, PartyId},
    prelude::{Abbreviate, Environment, Error, Protocol},
    protocol::ProtocolId,
    ss::LinearShare,
};

/// Protocol that **opens** (publicly reconstructs) a secret shared in a linear secret sharing
/// scheme `S`: every party reveals its share to every other party, and each party reconstructs
/// the secret locally with [`LinearShare::secret_from_shares`].
///
/// Each party sends its share to all peers (one round) and then collects one share from *every*
/// other party. Shares may arrive in any order, so reconstruction pairs each collected share with
/// the party it came from. Once a party holds all `n` shares — its own included — it reconstructs
/// and returns the secret.
///
/// # Security model: passive adversary
///
/// This protocol assumes a **passive (semi-honest) adversary**: every party follows the protocol,
/// so in particular every party in the network holds a share of the same secret and always sends
/// it. Under that assumption, waiting for a share from every peer is safe (nobody stays silent),
/// and the message pattern is exactly balanced — each party consumes precisely one share from
/// every other party, leaving nothing queued in the network. A party that crashes or withholds
/// its share is outside this model and blocks the protocol; lifting the assumption (receive
/// timeouts, identifiable abort) is planned malicious-model work — see `docs/roadmap.md` §11.
pub struct PassiveOpenShr<S> {
    /// The local party's share of the secret being opened.
    my_share: S,
}

impl<S> PassiveOpenShr<S>
where
    S: LinearShare,
{
    /// Creates the protocol instance for the local party holding `my_share`. Every party in the
    /// network must hold a share of the same secret and run this protocol (see the security model
    /// on [`PassiveOpenShr`]).
    pub fn new(my_share: S) -> Self {
        Self { my_share }
    }
}

impl<S, E> Protocol<E> for PassiveOpenShr<S>
where
    S: LinearShare + Abbreviate,
    E: Environment,
    S::Value: Sync + Send + 'static,
{
    type Output = S::Value;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let me = env.network().local_party();
        let parties = env.network().party_ids();

        // Reveal: send my own share to every other party.
        let mut messages = Vec::with_capacity(parties.len().saturating_sub(1));
        for party in parties.iter().filter(|&party| *party != me) {
            let mut pkt = Packet::empty();
            pkt.write_labeled(&self.my_share)?;
            messages.push((*party, pkt));
        }
        env.network_mut().send_many(&messages).await?;

        // Collect one share from every other party, in arrival order, starting from my own.
        let mut shares = vec![self.my_share];
        let mut senders = vec![me];
        while shares.len() < parties.len() {
            let (sender, mut pkt) = env.network_mut().recv_any().await?;
            let share: S = pkt.pop()?;
            shares.push(share);
            senders.push(sender);
        }
        let secret = S::secret_from_shares(&shares, &senders)?;
        Ok(secret)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PassiveOpenLinearShr")
    }
}

/// Protocol that opens a shared secret **towards a single party**: every party sends its share to
/// the designated `receiver`, and only the receiver reconstructs. The receiver's output is
/// `Some(secret)`; every other party's output is `None`.
///
/// This is the common *output* pattern of an MPC computation: the parties compute on shares and
/// reveal the result only to the party entitled to learn it. All parties construct the protocol
/// with the same [`new`](PassiveOpenToParty::new) call — the role each party plays is decided at
/// run time by comparing the designated `receiver` with the local party.
///
/// # Security model: passive adversary
///
/// Like [`PassiveOpenShr`], this protocol assumes a **passive (semi-honest) adversary**: every
/// party in the network holds a share of the same secret and always sends it, so the receiver can
/// safely wait for a share from every peer, and no message is left queued (the receiver consumes
/// exactly one share per peer; non-receivers send one message and finish). A party that crashes
/// or withholds its share is outside this model and blocks the receiver; lifting the assumption
/// (receive timeouts, identifiable abort) is planned malicious-model work — see
/// `docs/roadmap.md` §11.
pub struct PassiveOpenToParty<S> {
    /// The single party that learns the secret.
    receiver: PartyId,
    /// The local party's share of the secret being opened.
    my_share: S,
}

impl<S> PassiveOpenToParty<S>
where
    S: LinearShare,
{
    /// Creates the protocol instance for the local party holding `my_share`, opening the secret
    /// towards `receiver`. Every party in the network must hold a share of the same secret and
    /// run this protocol (see the security model on [`PassiveOpenToParty`]).
    pub fn new(receiver: PartyId, my_share: S) -> Self {
        Self { receiver, my_share }
    }
}

impl<S, E> Protocol<E> for PassiveOpenToParty<S>
where
    S: LinearShare + Abbreviate,
    E: Environment,
    S::Value: Sync + Send + 'static,
{
    type Output = Option<S::Value>;

    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let me = env.network().local_party();

        if self.receiver != me {
            // Not the receiver: reveal my share to the receiver and finish.
            let mut pkt = Packet::empty();
            pkt.write_labeled(&self.my_share)?;
            env.network_mut().send_to(self.receiver, &pkt).await?;
            Ok(None)
        } else {
            // The receiver: collect one share from every other party, starting from my own, and
            // reconstruct.
            let parties = env.network().party_ids();
            let mut shares = vec![self.my_share];
            let mut senders = vec![me];
            while shares.len() < parties.len() {
                let (sender, mut pkt) = env.network_mut().recv_any().await?;
                let share: S = pkt.pop()?;
                shares.push(share);
                senders.push(sender);
            }
            let secret = S::secret_from_shares(&shares, &senders)?;
            Ok(Some(secret))
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PassiveOpenToPartyLinearShr")
    }
}
