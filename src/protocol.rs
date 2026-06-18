use crate::net::{Network, NetworkError};
use async_trait::async_trait;
use thiserror::Error;

/// Error that may occur during a protocol execution.
#[derive(Debug, Error)]
#[non_exhaustive]
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
    async fn run(self, environment: &mut Environment<N>) -> Result<Self::Output, Error>;
    /// Identifier of the protocol.
    fn name(&self) -> &'static str;
}

/// Environment in which a protocol is executed.
///
/// The environment includes the network that is used in the protocol simulation.
pub struct Environment<N: Network> {
    /// Network in which the protocol is being executed.
    pub network: N,
}

impl<N: Network> Environment<N> {
    /// Creates a new environment.
    pub fn new(network: N) -> Self {
        Self { network }
    }
}
