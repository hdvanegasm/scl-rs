//! This example implements a simple send and receive protocol for two parties. In this protocol,
//! each party with ID `i` sends `i` to the party with ID `1 - i`.

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use scl_rs::{
    net::{simulation::channel::SimpleNetworkConfig, Network, Packet, PartyId},
    prelude::{simulate, Environment, Error, Protocol},
    protocol::{GeneralEnv, ProtocolId},
};

// A protocol is a struct that implements the `Protocol` trait.
pub struct SendRecvProtocol;

// The protocol is written generic over `E: Environment` — and therefore over any `Network` the
// environment wraps — instead of being tied to a concrete network. This is the key to scl-rs'
// portability: the very same protocol runs on the deterministic simulator (`SimNetwork`, used in
// `main` below) and over a real TLS deployment (`TcpNetwork`), without any changes to its logic.
#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for SendRecvProtocol {
    // Here, you define what is the output of the protocol. This output can be arbitrary: it may be
    // a share, a party ID, a stream of bytes, etc.
    type Output = usize;

    // The key method of a protocol is the `run` method. It is written from the perspective of the
    // current party, that is, you are writing how the current party will behave. Different parties
    // may behave in different ways, and you can access the current party ID using
    // `environment.network().local_party()`.
    async fn run(self, environment: &mut E) -> Result<usize, Error> {
        // You can only send `Packet`s to the network. A packet is a collection of serialized
        // objects. The packet can hold the serialized version of any serializable data structure.
        // Also, one packet can hold any number of completely different data structures as long as
        // the data structure implements `serde::Serialize` and `serde::Deserialize`.
        let mut packet = Packet::empty();

        // For example, here we are storing the current party ID into the packet to send it to the
        // network. Network operations return a `Result`, so we propagate any error with `?`.
        packet.write(&environment.network().local_party().as_usize())?;

        let other = environment.network().other()?;

        // Given that we are writing the protocol from the perspective of the current party, this
        // line instructs the current party to send its party ID to the other party.
        environment.network_mut().send_to(other, &packet).await?;

        // From the perspective of the other party, the party sends also its ID, so the current
        // party needs to receive it.
        let received_packet = environment.network_mut().recv_from(other).await?;

        environment.network_mut().close().await?;

        let their_id: usize = received_packet.read(0)?;

        // At the end, the protocol returns its result. In this case, the ID of the other party in
        // the protocol.
        Ok(their_id)
    }

    // Every protocol names itself with a `ProtocolId`. The simulator brackets each protocol run
    // with this id, so it labels the protocol's scope in the trace printed by `main` below — and
    // when a protocol calls another one, the nesting shows up as an indented block.
    fn id(&self) -> ProtocolId {
        ProtocolId::from("SendRecvProtocol")
    }
}

fn main() {
    let p0 = PartyId::from(0);
    let p1 = PartyId::from(1);

    // To simulate a protocol, you need to call the function `simulate` which returns a
    // `SimulationOutcome`. The function receives also a network configuration that specifies the
    // network parameters for each specific point-to-point connection. For this example, we will use
    // the built-in `SimpleNetworkConfig` where there are no delays between any two parties.
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| SendRecvProtocol,
        |_, net| GeneralEnv::new(net, ChaCha20Rng::from_rng(&mut rand::rng())),
        // The last argument is the list of hooks: callbacks that fire as each event is recorded, to
        // observe or steer the run. This protocol needs none. See `scl_rs::net::simulation::hook`.
        vec![],
    );

    // Once the protocol finishes, you can access the protocol traces and the outputs for each party.
    println!("=== P0 trace: ===\n{}", outcome.traces[&p0]);
    println!("=== P1 trace: ===\n{}", outcome.traces[&p1]);
    println!("P0 output: {}", outcome.outputs[&p0]);
    println!("P1 output: {}", outcome.outputs[&p1]);
}
