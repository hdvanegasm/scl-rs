use crate::net::simulation::channel::NetworkConfig;
use crate::net::simulation::event::Event;
use crate::net::simulation::hook::TriggeredHook;
use crate::net::simulation::network::Transport;
use crate::net::simulation::SimulationTrace;
use crate::net::{Network, PartyId};
use crate::protocol::Protocol;
use std::io::Write;
use std::sync::Arc;

/// The [`crate::net::simulation::Manager`] manages certain aspects of a simulation:
/// - The number of replications in the simulation.
/// - The protocol to simulate.
/// - What we do with the protocol output.
/// - What network to use.
/// - When to terminate the protocol.
/// - What to do when a protocol finishes.
pub trait Manager<C: NetworkConfig, N: Network>: Send + Sync {
    fn add_hook(&mut self, trigger_event: Event, hook: Arc<dyn TriggeredHook<C>>);
    fn add_unconditional_hook(&mut self, hook: Arc<dyn TriggeredHook<C>>);
    fn protocol(&self) -> Vec<Box<dyn Protocol<N>>>;
    fn handle_protocol_output(&mut self, party_id: PartyId, output: Vec<u8>);
    fn handle_simulator_output(&mut self, party_id: PartyId, trace: &SimulationTrace);
    fn network_config(&self) -> &C;
    fn hooks(&self) -> Vec<Arc<dyn TriggeredHook<C>>>;
}
