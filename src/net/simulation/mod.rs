use crate::net::simulation::event::Event;
use thiserror::Error;

pub mod channel;
pub mod context;
pub mod event;
pub mod hook;
pub mod transport;

#[derive(Error, Debug)]
pub enum SimulationError {
    #[error("a cancellation error occur")]
    CancellationError,
}

#[derive(Debug, Clone, PartialEq, Hash, PartialOrd)]
pub struct PartyId(usize);

/// Manager of a simulation.
///
/// The [`Manager`] manages certain aspects of a simulation:
/// - The number of replications in the simulation.
/// - The protocol to simulate.
/// - What we do with the protocol output.
/// - What network to use.
/// - When to terminate the protocol.
/// - What to do when a protocol finishes.
pub trait Manager {
    fn protocol();
    fn handle_simulator_output();
    fn network_configuration();
    fn handle_protocol_output();
    fn add_hook();
    fn simulate();
}

/// Transport layer for a simulated network.
pub struct Transport;

/// A hook is a piece of code that is run in response to an event.
pub trait Hook {
    fn run();
}

/// Trace of events that occur in the simulation.
pub struct SimulationTrace(Vec<Event>);

/// Simulates
pub fn simulate<M: Manager>(manager: M) {
    todo!()
}
