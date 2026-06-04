use crate::net::simulation::channel::{ChannelConfigBuilder, ChannelId};
use crate::net::simulation::event::{Event, EventType};
use crate::net::{NetworkError, PartyId};
use thiserror::Error;
use tokio::task::JoinError;

/// Implementation of the simulated channels.
pub mod channel;

/// Context of the simulation.
pub mod context;

/// Simulation events.
pub mod event;

/// Hook mechanism to run during the simulation.
pub mod hook;

/// Implement traits to manage protocol executions.
pub mod manager;

/// Implement the network simulation.
pub mod network;

/// Implements tools to run protocol simulations and manage the results.
pub mod simulator;

/// Errors for protocol simulations.
#[derive(Error, Debug)]
pub enum SimulationError {
    /// The execution was interrupted and cancelled.
    #[error("a cancellation error occurred")]
    CancellationError,
    /// Error locking resources for the async execution.
    #[error("error locking the resource: {0:?}")]
    SyncError(String),
    /// Error joining concurrent tasks.
    #[error("error running the protocol concurrently: {0:?}")]
    JoinHandleError(#[from] JoinError),
    /// Error in the simulated network.
    #[error("network error: {0:?}")]
    NetworkError(#[from] NetworkError),
    /// The party was not found in a certain set or collection.
    #[error("party {0:?} not found")]
    PartyNotFound(PartyId),
    /// The channel was not in a certain set or collection.
    #[error("channel {id:?} not found: {err_context}")]
    ChannelNotFound {
        /// ID of the missing channel.
        id: ChannelId,
        /// Context for the error.
        err_context: &'static str,
    },
    /// The set of sent messages is empty.
    #[error("sends are empty")]
    SendsEmpty,
    /// Invalid configuration for the simulated channels.
    #[error("invalid configuration parameters for the channel: {0:?}")]
    InvalidConfig(ChannelConfigBuilder),
}

/// Specific result type for the [`SimulationError`].
pub type Result<T> = std::result::Result<T, SimulationError>;

/// Trace of events that occur in the simulation.
#[derive(Debug, Clone)]
pub struct SimulationTrace(Vec<Event>);

impl SimulationTrace {
    /// Creates an empty simulation trace.
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    /// Creates a simulation trace with the list of `events`.
    pub fn new(events: Vec<Event>) -> Self {
        Self(events)
    }

    /// Returns the number of events in the simulation trace.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns a slice of the events in the trace.
    pub fn events(&self) -> &[Event] {
        &self.0
    }

    /// Adds an event to the simulation trace.
    pub fn add_event(&mut self, event: Event) {
        self.0.push(event);
    }

    /// Returns the event types currently stored in the trace.
    pub fn event_types(&self) -> Vec<EventType> {
        let mut event_types = Vec::new();
        for event in &self.0 {
            event_types.push(event.event_type());
        }
        event_types
    }
}
