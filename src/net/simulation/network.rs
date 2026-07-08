//! The [`Network`](crate::net::Network) implementation backed by the deterministic simulator.
//!
//! [`SimNetwork`](crate::net::simulation::network::SimNetwork) is the per-party handle the simulator hands to a protocol in place of a real
//! socket: every `send`/`recv` routes through the shared
//! [`Switchboard`](crate::net::simulation::switchboard::Switchboard), and receives with no message
//! ready suspend the party (through the switchboard's `recv` futures) so the executor can advance
//! virtual time. See [`SimNetwork`](crate::net::simulation::network::SimNetwork) for the full model.

use crate::net;
use crate::net::simulation::event::Event;
use crate::net::simulation::switchboard::recv::RecvAnyTimeout;
use crate::net::simulation::switchboard::{
    recv::{Recv, RecvAny, RecvTimeout},
    Switchboard,
};
use crate::net::{Network, NetworkError, Packet, PartyId};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

/// The [`Network`] implementation backed by the deterministic simulator.
///
/// Every party runs the *same* protocol code it would run in a real deployment; the only
/// difference is that `send_to`/`recv_from`/`recv_any` route through a shared [`Switchboard`] instead
/// of a TCP socket. A `recv_from` (or `recv_any`) whose message has not arrived yet suspends the
/// party (returns `Poll::Pending`), letting the executor advance virtual time to the next
/// deliverable event.
///
/// One `SimNetwork` is created per party by [`simulate`](crate::net::simulation::simulator::simulate);
/// all parties share the same `Switchboard`. The `Mutex` is uncontended (the core is
/// single-threaded) and exists only to satisfy the `Send` bound on the [`Network`] trait.
pub struct SimNetwork {
    /// ID of the local party.
    local: PartyId,
    /// IDs of all parties participating in the simulation.
    parties: Vec<PartyId>,
    /// The shared in-memory router all parties communicate through.
    switchboard: Arc<std::sync::Mutex<Switchboard>>,
}

impl SimNetwork {
    /// Creates a `SimNetwork` for the `local` party, knowing the full `parties` set and sharing the
    /// given `switchboard` with every other party in the simulation.
    pub fn new(
        local: PartyId,
        parties: Vec<PartyId>,
        switchboard: Arc<std::sync::Mutex<Switchboard>>,
    ) -> Self {
        Self {
            local,
            parties,
            switchboard,
        }
    }
}

#[async_trait]
impl Network for SimNetwork {
    fn local_party(&self) -> PartyId {
        self.local
    }

    async fn recv_any(&mut self) -> net::Result<(PartyId, Packet)> {
        Ok(RecvAny::new(self.switchboard.clone(), self.local, self.parties.clone()).await)
    }

    async fn recv_any_with_timeout(&mut self, timeout: Duration) -> net::Result<(PartyId, Packet)> {
        RecvAnyTimeout::new(
            self.switchboard.clone(),
            self.local,
            self.parties.clone(),
            timeout,
        )
        .await
    }

    async fn send_to(&mut self, party_id: PartyId, packet: &Packet) -> net::Result<usize> {
        self.switchboard.lock().expect("lock must be free").send(
            self.local,
            party_id,
            packet.clone(),
        );
        Ok(packet.size())
    }

    async fn recv_from(&mut self, party_id: PartyId) -> net::Result<Packet> {
        let packet = Recv::new(self.switchboard.clone(), party_id, self.local).await;
        Ok(packet)
    }

    async fn recv_from_with_timeout(
        &mut self,
        party_id: PartyId,
        timeout: Duration,
    ) -> net::Result<Packet> {
        RecvTimeout::new(self.switchboard.clone(), party_id, self.local, timeout).await
    }

    fn other(&self) -> net::Result<PartyId> {
        if self.parties.len() != 2 {
            Err(NetworkError::ExpectedTwoNodeNet(self.parties.len()))
        } else {
            Ok(PartyId::from(1 - self.local.as_usize()))
        }
    }

    fn party_ids(&self) -> Vec<PartyId> {
        self.parties.clone()
    }

    async fn close(&mut self) -> net::Result<()> {
        Ok(())
    }

    fn record_protocol_begin(&mut self, protocol_name: &'static str) {
        let mut switchboard = self.switchboard.lock().expect("lock must be free");
        let timestamp = switchboard.clock_of(self.local);
        switchboard.record_event(
            self.local,
            Event::ProtocolBegin {
                timestamp,
                protocol_name,
            },
        );
    }

    fn record_protocol_end(&mut self, protocol_name: &'static str) {
        let mut switchboard = self.switchboard.lock().expect("lock must be free");
        let timestamp = switchboard.clock_of(self.local);
        switchboard.record_event(
            self.local,
            Event::ProtocolEnd {
                timestamp,
                protocol_name,
            },
        );
    }
}
