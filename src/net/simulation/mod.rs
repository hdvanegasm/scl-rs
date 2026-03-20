use crate::net::simulation::channel::{ChannelConfigBuilder, ChannelId};
use crate::net::simulation::event::{Event, EventType};
use crate::net::{NetworkError, PartyId};
use thiserror::Error;
use tokio::task::JoinError;

pub mod channel;
pub mod context;
pub mod event;
pub mod hook;
pub mod manager;
pub mod network;
pub mod simulator;

#[derive(Error, Debug)]
pub enum SimulationError {
    #[error("A cancellation error occurred")]
    CancellationError,
    #[error("Error locking the resource: {0:?}")]
    SyncError(String),
    #[error("Error running the protocol concurrently: {0:?}")]
    JoinHandleError(#[from] JoinError),
    #[error("Network error: {0:?}")]
    NetworkError(#[from] NetworkError),
    #[error("Party {0:?} not found")]
    PartyNotFound(PartyId),
    #[error("Channel {id:?} not found: {err_context}")]
    ChannelNotFound {
        id: ChannelId,
        err_context: &'static str,
    },
    #[error("Sends are empty")]
    SendsEmpty,
    #[error("invalid configuration parameters for the channel: {0:?}")]
    InvalidConfig(ChannelConfigBuilder),
}

pub type Result<T> = std::result::Result<T, SimulationError>;

/// Trace of events that occur in the simulation.
#[derive(Debug, Clone)]
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

    pub fn events(&self) -> &[Event] {
        &self.0
    }

    pub fn add_event(&mut self, event: Event) {
        self.0.push(event);
    }

    pub fn event_types(&self) -> Vec<EventType> {
        let mut event_types = Vec::new();
        for event in &self.0 {
            event_types.push(event.event_type());
        }
        event_types
    }
}
