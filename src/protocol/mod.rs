/// Generic secret-sharing protocols (dealing and opening) over any [`LinearShare`](crate::ss::LinearShare) scheme.
pub mod share;

pub mod passive_shamir;

use crate::{
    net::{Network, NetworkError},
    prelude::Ring,
    ss::ShareError,
};
use rand::CryptoRng;
use std::fmt;
use std::future::Future;
use thiserror::Error;

/// Error that may occur during a protocol execution.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// A network operation failed during the protocol.
    #[error("network error: {0:?}")]
    Network(#[from] NetworkError),
    /// A secret-sharing operation (dealing or reconstructing) failed during the protocol.
    ///
    /// The boxed source is a [`ShareError<T>`](crate::ss::ShareError) with the ring type erased, so
    /// that this enum — and therefore the [`Protocol`] trait — stays independent of any particular
    /// ring. Callers that need the structured error can downcast the box to the concrete
    /// `ShareError<T>`.
    #[error("share error: {0}")]
    Share(Box<dyn std::error::Error + Send + Sync>),
    /// The protocol was constructed with input that does not match the role it is asked to play —
    /// for example, running as the dealer of [`PassiveDealShr`](share::deal::PassiveDealShr)
    /// without providing a secret.
    #[error("the input is not well formed for the current protocol")]
    Input,
}

impl<T> From<ShareError<T>> for Error
where
    T: Ring + Send + Sync + 'static,
{
    fn from(value: ShareError<T>) -> Self {
        Error::Share(Box::new(value))
    }
}

/// The name a protocol reports for itself, used to label its scope in a trace.
///
/// A protocol returns its id from [`Protocol::id`]. [`Protocol::execute`] then hands that id to
/// [`Network::record_protocol_begin`] and [`Network::record_protocol_end`], which the simulator
/// turns into the [`ProtocolBegin`](crate::net::simulation::event::Event::ProtocolBegin) and
/// [`ProtocolEnd`](crate::net::simulation::event::Event::ProtocolEnd) events that bracket the
/// protocol's scope in the trace.
///
/// A `ProtocolId` wraps a `&'static str`: a protocol's name is fixed at compile time rather than
/// built per instance, which keeps the id `Copy` and allocation-free on the tracing path. Build one
/// from a string literal with [`From`], and render it with [`Display`](fmt::Display):
///
/// ```rust
/// use scl_rs::protocol::ProtocolId;
///
/// let id = ProtocolId::from("PassiveOpenLinearShr");
/// assert_eq!(id.to_string(), "PassiveOpenLinearShr");
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Hash)]
pub struct ProtocolId(&'static str);

impl From<ProtocolId> for String {
    fn from(value: ProtocolId) -> Self {
        String::from(value.0)
    }
}

impl From<&'static str> for ProtocolId {
    fn from(value: &'static str) -> Self {
        ProtocolId(value)
    }
}

impl fmt::Display for ProtocolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a protocol.
pub trait Protocol<E: Environment>: Send + Sync {
    /// The output of the protocol.
    type Output;
    /// Behavior of the protocol when run.
    ///
    /// Declared as `fn … -> impl Future + Send` rather than `async fn` so the returned future is
    /// guaranteed [`Send`] and a protocol can be driven on a multi-threaded runtime; implementors
    /// still write a plain `async fn run`.
    fn run(self, environment: &mut E) -> impl Future<Output = Result<Self::Output, Error>> + Send;
    /// Identifier of the protocol, used to label its scope in a trace. See [`ProtocolId`].
    fn id(&self) -> ProtocolId;

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
    fn execute(
        self,
        environment: &mut E,
    ) -> impl Future<Output = Result<Self::Output, Error>> + Send
    where
        Self: Sized,
    {
        async move {
            let id = self.id();
            environment.network_mut().record_protocol_begin(id);
            let output = self.run(environment).await;
            environment.network_mut().record_protocol_end(id);
            output
        }
    }
}

/// Environment that holds all the information needed across multiple composability layers.
///
/// This is information that is needed throughout the entire session no matter how deep you go in the
/// composability layers. For example, a common case that is needed (and required) across all
/// protocol and sub-protocol calls (in every composability layer) is the network. Hence, we enforce
/// that the environment should hold the object that implements the [`Network`] trait, as the same
/// network is used across all the protocols and sub-protocols.
pub trait Environment: Send {
    /// Network type that will be used to run the protocol.
    type Net: Network;

    /// Returns a mutable reference to the network used for the protocol execution.
    fn network_mut(&mut self) -> &mut Self::Net;
    /// Returns an immutable reference to the network used for the protocol execution.
    fn network(&self) -> &Self::Net;
}

/// An [`Environment`] that additionally carries the session's cryptographically secure RNG.
///
/// Protocols that sample secret material — dealing shares, generating correlated randomness —
/// bound their environment on this trait and draw from [`rng_mut`](RandEnvironment::rng_mut)
/// instead of a global generator. With per-party seeded RNGs, a simulated protocol run is
/// reproducible end to end; protocols that never sample should bound on plain [`Environment`] so
/// they don't over-constrain their callers.
pub trait RandEnvironment: Environment {
    /// The RNG used for all sampling in this session. The [`CryptoRng`] bound keeps secret
    /// material from being derived from a predictable (non-cryptographic) generator.
    type Rng: CryptoRng + Send;

    /// Returns a mutable reference to the session RNG.
    fn rng_mut(&mut self) -> &mut Self::Rng;
}

/// Environment that provides the network and the session RNG that traverse through layers.
///
/// This is the most basic environment: it implements both [`Environment`] and
/// [`RandEnvironment`]. If a protocol requires further information that needs to be passed to deep
/// layers of protocol composition, we recommend the API user to create a different struct that
/// implements the [`Environment`] trait.
pub struct GeneralEnv<N: Network, R: CryptoRng + Send> {
    /// Network in which the protocol is being executed.
    pub network: N,
    /// Session RNG used by protocols that sample secret material (see [`RandEnvironment`]).
    pub rng: R,
}

impl<N: Network, R: CryptoRng + Send> GeneralEnv<N, R> {
    /// Creates a new general environment.
    pub fn new(network: N, rng: R) -> Self {
        Self { network, rng }
    }
}

impl<R: CryptoRng + Send, N: Network> Environment for GeneralEnv<N, R> {
    type Net = N;

    fn network(&self) -> &Self::Net {
        &self.network
    }

    fn network_mut(&mut self) -> &mut Self::Net {
        &mut self.network
    }
}

impl<R: CryptoRng + Send, N: Network> RandEnvironment for GeneralEnv<N, R> {
    type Rng = R;
    fn rng_mut(&mut self) -> &mut Self::Rng {
        &mut self.rng
    }
}
