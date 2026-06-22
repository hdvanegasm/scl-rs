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

    /// Runs this protocol bracketed by protocol-scope trace markers.
    ///
    /// This is the entry point for invoking a protocol — including a sub-protocol called from
    /// within another protocol's [`run`](Protocol::run). It records a protocol-begin marker,
    /// runs the protocol, and records a protocol-end marker. Those markers let the trace reflect
    /// how protocols nest (see the tree-formatted
    /// [`SimulationTrace`](crate::net::simulation::SimulationTrace) display). On backends that keep
    /// no trace (a real network), the markers are no-ops, so behavior is identical to calling
    /// [`run`](Protocol::run) directly.
    ///
    /// Prefer calling `execute` over `run`: `run` defines a protocol's behavior, while `execute`
    /// invokes it with tracing.
    async fn execute(self, environment: &mut E) -> Result<Self::Output, Error>
    where
        Self: Sized,
    {
        let name = self.name();
        environment.network_mut().record_protocol_begin(name);
        let output = self.run(environment).await;
        environment.network_mut().record_protocol_end(name);
        output
    }
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
