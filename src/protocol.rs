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
pub trait Protocol<E: Environment>: Send + Sync {
    /// The output of the protocol.
    type Output;
    /// Behavior of the protocol when run.
    async fn run(self, environment: &mut E) -> Result<Self::Output, Error>;
    /// Identifier of the protocol.
    fn name(&self) -> &'static str;
}

/// Environment that holds all the information needed accross multiple composability layers.
///
/// This is information that is needed throughout the entire session no mather how deep you go in the
/// composability layers. For example, a common case that is needed (and required) across all
/// protocol and sub-protocol calls (in every composability layer) is the network. Hence, we enforce
/// that the environment should hold the object that implements the [`Network`] trait, as the same
/// network is used across all the protocols and sub-protocols.
pub trait Environment: Send {
    /// Network type that will be used to run the protocol.
    type Net: Network;

    /// Returns a mutable reference to the network used for the protocol execution.
    fn network_mut(&mut self) -> &mut Self::Net;
    /// Returns an inmutable reference to the network used for the protocol execution.
    fn network(&self) -> &Self::Net;
}

/// Environment that provides the network as the sole information that traverses through layers.
///
/// This is the most basic environment. If a protocol requires information that needs to be passed
/// to deep layers of protocol composition, we recommend the API user to create a diferent struct that
/// implements the [`Environment`] trait.
pub struct GeneralEnv<N: Network> {
    /// Network in which the protocol is being executed.
    pub network: N,
}

impl<N: Network> GeneralEnv<N> {
    /// Creates a new general environment.
    pub fn new(network: N) -> Self {
        Self { network }
    }
}

impl<N: Network> Environment for GeneralEnv<N> {
    type Net = N;

    fn network(&self) -> &Self::Net {
        &self.network
    }

    fn network_mut(&mut self) -> &mut Self::Net {
        &mut self.network
    }
}
