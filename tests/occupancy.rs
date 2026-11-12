//! Link occupancy on the deterministic simulator.
//!
//! A link serializes one message at a time. A message holds the link for its serialization time
//! (its size over the channel's throughput), and then travels for its propagation time (half the
//! RTT), which holds nothing. So two messages sent at the same instant on one link arrive one
//! serialization time apart, and a message on another link is not delayed by either.
//!
//! The assertions are exact (`assert_eq!` on `Duration`), not approximate: the switchboard adds the
//! same `Duration`s that the tests read from [`ChannelConfig::transmission`].
//!
//! [`ChannelConfig::transmission`]: scl_rs::net::simulation::channel::ChannelConfig::transmission

use std::time::Duration;

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    net::{
        simulation::{
            channel::{Link, NetworkConfig, SimpleNetworkConfig},
            network::SimNetwork,
            simulator::simulate,
            switchboard::TransmissionTime,
        },
        Network, Packet, PartyId,
    },
    protocol::{
        concurrent::Join, Environment, Error, ForkEnvironment, GeneralEnvironment, Protocol,
        ProtocolId,
    },
};

const MIB: usize = 1024 * 1024;

fn party(index: usize) -> PartyId {
    PartyId::from(index)
}

fn env(party: PartyId, network: SimNetwork) -> GeneralEnvironment<SimNetwork, ChaCha20Rng> {
    GeneralEnvironment::new(network, ChaCha20Rng::seed_from_u64(party.as_usize() as u64))
}

fn mib_packet() -> Packet {
    let mut packet = Packet::empty();
    packet.write(&vec![0u8; MIB]).unwrap();
    packet
}

/// Party 0 sends 1 MiB to `target`, which receives it. Every other party does nothing.
struct Transfer {
    target: PartyId,
}

impl<E: Environment> Protocol<E> for Transfer {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        let local = env.network().local_party();
        if local == party(0) {
            env.network_mut()
                .send_to(self.target, &mib_packet())
                .await?;
        } else if local == self.target {
            env.network_mut().recv_from(party(0)).await?;
        }
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Transfer")
    }
}

/// Two transfers to party 1 and one to party 2, all at once.
///
/// Every party runs the same `Join` with the same three branches, so every party performs the same
/// sequence of forks; a party that is neither sender nor target runs a branch that returns at
/// once. The transfer to party 2 is the control: sent at the same instant on another link, it must
/// arrive as if nothing else were being sent.
struct ThreeTransfers;

impl<E: Environment + ForkEnvironment> Protocol<E> for ThreeTransfers {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        Join((
            Transfer { target: party(1) },
            Transfer { target: party(1) },
            Transfer { target: party(2) },
        ))
        .execute(env)
        .await?;
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("ThreeTransfers")
    }
}

/// Party 0 sends two 1 MiB packets to party 1 one after the other, at the same instant.
struct BackToBack;

impl<E: Environment> Protocol<E> for BackToBack {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        let network = env.network_mut();
        if network.local_party() == party(0) {
            network.send_to(party(1), &mib_packet()).await?;
            network.send_to(party(1), &mib_packet()).await?;
        } else {
            network.recv_from(party(0)).await?;
            network.recv_from(party(0)).await?;
        }
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("BackToBack")
    }
}

/// The serialization and propagation time of one 1 MiB packet on the link 0 -> `to`.
fn one_packet(config: &SimpleNetworkConfig, to: usize) -> (Duration, Duration) {
    let time = config
        .channel_config(Link::new(party(0), party(to)))
        .transmission(mib_packet().size());
    (time.serialization(), time.propagation())
}

/// Two packets sent at the same instant on one link share it. The second arrives one
/// serialization time after the first, not together with it. A packet sent at that instant on
/// another link is not delayed.
#[test]
fn concurrent_packets_on_one_link_share_its_bandwidth() {
    let config = SimpleNetworkConfig::wan();
    let (serialization, propagation) = one_packet(&config, 1);
    assert!(
        serialization > Duration::from_millis(10),
        "the test needs a visible serialization time"
    );

    let parties = vec![party(0), party(1), party(2)];
    let outcome = simulate(config.clone(), parties, |_| ThreeTransfers, env, vec![]);
    let metrics = outcome.metrics();
    assert_eq!(
        metrics.elapsed_time_for(party(1)).unwrap(),
        2 * serialization + propagation,
        "the second packet on the link 0 -> 1"
    );
    assert_eq!(
        metrics.elapsed_time_for(party(2)).unwrap(),
        serialization + propagation,
        "the only packet on the link 0 -> 2"
    );
}

/// The same holds within one session: the link is shared by whatever is sent on it.
#[test]
fn back_to_back_packets_on_one_link_share_its_bandwidth() {
    let config = SimpleNetworkConfig::wan();
    let (serialization, propagation) = one_packet(&config, 1);

    let outcome = simulate(
        config.clone(),
        vec![party(0), party(1)],
        |_| BackToBack,
        env,
        vec![],
    );
    assert_eq!(
        outcome.metrics().elapsed_time_for(party(1)).unwrap(),
        2 * serialization + propagation
    );
}

/// The two terms add up to the delay of a packet sent alone, exactly: a packet with the link to
/// itself arrives when it did before occupancy was modelled.
#[test]
fn serialization_and_propagation_add_up_to_the_message_delay() {
    let config = SimpleNetworkConfig::wan();
    for size in [0, 1, 1460, 1461, 65_536, MIB] {
        let channel = config.channel_config(Link::new(party(0), party(1)));
        let time = channel.transmission(size);
        assert_eq!(
            time.serialization() + time.propagation(),
            channel.message_delay(size),
            "size {size}"
        );
    }
    let own = config.channel_config(Link::new(party(0), party(0)));
    assert_eq!(
        own.transmission(MIB),
        TransmissionTime::new(Duration::ZERO, Duration::ZERO)
    );
}
