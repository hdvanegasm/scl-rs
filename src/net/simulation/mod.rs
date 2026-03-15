use crate::net::simulation::channel::ChannelId;
use crate::net::simulation::event::Event;
use thiserror::Error;

pub mod channel;
pub mod context;
pub mod event;
pub mod hook;
pub mod manager;
pub mod simulator;
pub mod transport;

#[derive(Error, Debug)]
pub enum SimulationError {
    #[error("A cancellation error occurred")]
    CancellationError,
    #[error("Channel ID not found: {0:?}")]
    ChannelIdNotFound(ChannelId),
    #[error("Error locking the resource: {0:?}")]
    SyncError(String),
}

/// Trace of events that occur in the simulation.
#[derive(Debug)]
pub struct SimulationTrace(Vec<Event>);

impl SimulationTrace {
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn new(events: Vec<Event>) -> Self {
        Self(events)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}
