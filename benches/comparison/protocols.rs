//! The two protocols the comparison exercises, one per network regime.
//!
//! Both are written once against [`Environment`] and run unchanged on the deterministic simulator
//! ([`SimNetwork`](scl_rs::net::simulation::network::SimNetwork)) and on a real mutually
//! authenticated TLS deployment ([`TcpNetwork`](scl_rs::net::TcpNetwork)) — that portability is
//! precisely the property being measured, so neither protocol may suspend through anything but the
//! [`Network`] trait.
//!
//! They are deliberately synthetic. A real MPC protocol folds local compute time into the wall
//! clock, and the simulator does not model compute at all, so a gap between the two backends could
//! not be attributed to the network model. These two do nothing but move bytes.
//!
//! - [`PingPong`] is **round-dominated**: `rounds` sequential round trips of a small payload, so
//!   its running time is `rounds * RTT` plus a serialization term that shrinks with the payload.
//!   It is sensitive to latency and nearly blind to bandwidth.
//! - [`BulkTransfer`] is **bandwidth-dominated**: one large message plus a tiny acknowledgement,
//!   so its running time is `bytes * 8 / throughput` plus a single round trip. It is sensitive to
//!   throughput and nearly blind to latency.
//!
//! Party 0 is the initiator in both, and is the party whose timing is the headline figure; party 1
//! mirrors it. Both parties run the same protocol value and branch on
//! [`local_party`](Network::local_party), as the simulator requires a single protocol type across
//! the run.

use scl_rs::{
    net::{Network, Packet},
    prelude::{Environment, Error, Protocol},
    protocol::ProtocolId,
};

/// Payload of the acknowledgement [`BulkTransfer`] sends back, in bytes. Small enough that its
/// serialization time is negligible against the bulk message at every bandwidth measured here, so
/// the acknowledgement costs essentially half a round trip.
const ACK_BYTES: usize = 1;

/// Payload of the barrier message exchanged before timing starts. See [`barrier`].
const BARRIER_BYTES: usize = 1;

/// The round-dominated protocol: `rounds` sequential round trips of a `payload_bytes` payload.
///
/// Party 0 sends a payload and waits for party 1 to echo it back, `rounds` times in sequence. No
/// message is ever in flight concurrently with another, so the running time is a sum of one-way
/// delays and the protocol is a direct probe of the model's latency term.
pub struct PingPong {
    /// Number of sequential round trips.
    pub rounds: usize,
    /// Size of the payload carried in each direction, in bytes.
    pub payload_bytes: usize,
}

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for PingPong {
    /// Total bytes this party received over the run, summed across rounds.
    type Output = usize;

    async fn run(self, environment: &mut E) -> Result<usize, Error> {
        let me = environment.network().local_party().as_usize();
        let other = environment.network().other()?;
        let mut received = 0;

        if me == 0 {
            let payload = vec![0u8; self.payload_bytes];
            let mut packet = Packet::empty();
            packet.write(&payload)?;

            for _ in 0..self.rounds {
                environment.network_mut().send_to(other, &packet).await?;
                let echo = environment.network_mut().recv_from(other).await?;
                received += echo.size();
            }
        } else {
            // Echoing the received packet rather than building a fresh one keeps both directions
            // byte-for-byte identical, so a round costs exactly one RTT plus two equal
            // serialization terms.
            for _ in 0..self.rounds {
                let ping = environment.network_mut().recv_from(other).await?;
                received += ping.size();
                environment.network_mut().send_to(other, &ping).await?;
            }
        }

        Ok(received)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("PingPong")
    }
}

/// The bandwidth-dominated protocol: one large message, acknowledged.
///
/// Party 0 sends `payload_bytes` in a single message and waits for party 1's [`ACK_BYTES`]
/// acknowledgement, which is what lets party 0 observe completion. The acknowledgement adds half a
/// round trip; everything else is serialization, so the protocol is a direct probe of the model's
/// throughput term.
pub struct BulkTransfer {
    /// Size of the single bulk message, in bytes.
    pub payload_bytes: usize,
}

#[async_trait::async_trait]
impl<E: Environment> Protocol<E> for BulkTransfer {
    /// Total bytes this party received over the run.
    type Output = usize;

    async fn run(self, environment: &mut E) -> Result<usize, Error> {
        let me = environment.network().local_party().as_usize();
        let other = environment.network().other()?;

        if me == 0 {
            let mut packet = Packet::empty();
            packet.write(&vec![0u8; self.payload_bytes])?;
            environment.network_mut().send_to(other, &packet).await?;
            let ack = environment.network_mut().recv_from(other).await?;
            Ok(ack.size())
        } else {
            let bulk = environment.network_mut().recv_from(other).await?;
            let mut ack = Packet::empty();
            ack.write(&vec![0u8; ACK_BYTES])?;
            environment.network_mut().send_to(other, &ack).await?;
            Ok(bulk.size())
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("BulkTransfer")
    }
}

/// Exchanges one tiny round trip so both parties leave with a warm, fully established connection.
///
/// A real run starts its stopwatch after this returns. Without it the measured span would absorb
/// however much of the TLS handshake happened to still be in flight, which the simulator does not
/// model and which varies far more than the protocol itself under a lossy shape. It lives here
/// rather than inside a protocol on purpose: a protocol runs on both backends, and adding a round
/// trip to the simulated run would shift the very number being compared.
pub async fn barrier<E: Environment>(environment: &mut E) -> Result<(), Error> {
    let me = environment.network().local_party().as_usize();
    let other = environment.network().other()?;
    let mut packet = Packet::empty();
    packet.write(&vec![0u8; BARRIER_BYTES])?;

    if me == 0 {
        environment.network_mut().send_to(other, &packet).await?;
        environment.network_mut().recv_from(other).await?;
    } else {
        environment.network_mut().recv_from(other).await?;
        environment.network_mut().send_to(other, &packet).await?;
    }

    Ok(())
}
