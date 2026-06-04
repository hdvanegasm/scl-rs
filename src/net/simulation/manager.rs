use crate::net::simulation::channel::NetworkConfig;
use crate::net::simulation::event::Event;
use crate::net::simulation::hook::TriggeredHook;
use crate::net::simulation::SimulationTrace;
use crate::net::{Network, PartyId};
use crate::protocol::Protocol;
use std::sync::Arc;

/// Manages certain aspects of a simulation like:
/// - The protocols to simulate.
/// - What we do with the protocol output.
/// - What network to use.
/// - The hooks triggered during the simulation.
/// - What to do at the end of the simulation.
pub trait Manager<C: NetworkConfig, N: Network>: Send + Sync {
    /// Adds a hook to the simulation conditioned to a trigger event.
    fn add_hook(&mut self, trigger_event: Event, hook: Arc<dyn TriggeredHook<C>>);
    /// Adds a hook to the simulation without a conditional trigger event.
    fn add_unconditional_hook(&mut self, hook: Arc<dyn TriggeredHook<C>>);
    /// Return the protocols that are executing.
    fn protocol(&self) -> Vec<Box<dyn Protocol<N>>>;
    /// Handles output of the executed protocols.
    fn handle_protocol_output(&mut self, party_id: PartyId, output: Vec<u8>);
    /// Handles the output at the end of the simulation.
    fn handle_simulator_output(&mut self, party_id: PartyId, trace: &SimulationTrace);
    /// Returns the network configuration that is used in the current simulation.
    fn network_config(&self) -> &C;
    /// Return the list of hooks registered to the current manager.
    fn hooks(&self) -> Vec<Arc<dyn TriggeredHook<C>>>;
}
