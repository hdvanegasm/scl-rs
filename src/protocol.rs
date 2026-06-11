use crate::net::{Network, NetworkError};
use async_trait::async_trait;
use thiserror::Error;

/// Error that may occur during a protocol execution.
#[derive(Debug, Error)]
pub enum Error {
    /// A network operation failed during the protocol.
    #[error("network error: {0:?}")]
    Network(#[from] NetworkError),
}

/// Represents a protocol.
#[async_trait]
pub trait Protocol<N: Network>: Send + Sync {
    /// The output of the protocol.
    type Output;
    /// Behavior of the protocol when run.
    async fn run(&self, environment: &mut Environment<N>) -> Result<Self::Output, Error>;
    /// Identifier of the protocol.
    fn name(&self) -> &'static str;
}

/// Clock that counts the elapsed time from a start point.
pub struct Clock {
    /// Instant in time in which the protocol starts to count.
    start: std::time::Instant,
}

impl Clock {
    /// Elapsed time since the protocol started to run.
    pub fn read(&self) -> std::time::Duration {
        self.start.elapsed()
    }

    /// Creates a new clock starting at the current instant.
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

/// Environment in which a protocol is executed.
///
/// The environment includes a clock counting the duration since the simulation started, and the
/// network that is used in the protocol simulation.
pub struct Environment<N: Network> {
    /// Network in which the protocol is being executed.
    pub network: N,
    clock: Clock,
}

impl<N: Network> Environment<N> {
    /// Creates a new environment.
    pub fn new(network: N) -> Self {
        Self {
            network,
            clock: Clock::new(),
        }
    }

    /// Returns a reference to the wall clock for the protocol execution.
    pub fn clock(&self) -> &Clock {
        &self.clock
    }
}
