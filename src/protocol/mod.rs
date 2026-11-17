/// Generic secret-sharing protocols (dealing and opening) over any [`LinearShare`](crate::ss::LinearShare) scheme.
pub mod share;

/// Concurrent composition: running independent sub-protocols at the same time within one party.
pub mod concurrent;

/// Zero-knowledge proofs, run as two-party [`Protocol`]s between a prover and a verifier.
pub mod zk;

pub mod passive_shamir;

use crate::{
    net::{ForkNetwork, Network},
    protocol::concurrent::BoxedProtocol,
};
use rand::{CryptoRng, SeedableRng};
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;

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
///
/// # Cancel safety
///
/// The future of a protocol, from [`run`](Protocol::run) or [`execute`](Protocol::execute), is in
/// general **not** cancel safe, even when every network call it makes is (see [Cancel
/// safety](Network#cancel-safety) on `Network`). A protocol is a sequence of rounds that the
/// parties run in step. If a party drops the future part-way:
///
/// - the protocol cannot be resumed, since `run` and `execute` consume it;
/// - the other parties may wait for messages that never come, until their own timeout or error;
/// - the messages the other parties already sent to this party stay in the protocol's session,
///   where the next receive in that session takes them in place of the ones it expects;
/// - its trace scope stays open, since `execute` never records its end.
///
/// To make a protocol safe to drop, for example to bound it in time or to race it against another
/// future, run it in a session of its own: fork the environment with [`ForkEnvironment::fork`],
/// execute the protocol on the child, and drop the child together with the protocol's future. The
/// leftover messages then belong to the child's session, where the caller's receives never look,
/// and the caller's session is unaffected. Every party must make that fork too, whether or not it
/// drops the protocol (see [Every party forks
/// alike](crate::net::ForkNetwork#every-party-forks-alike)). The combinators of [`concurrent`] run
/// each sub-protocol this way. Like them, run nothing else in the caller's session until the
/// protocol on the child has returned or been dropped (see [Fork only through the
/// combinators](concurrent#fork-only-through-the-combinators)).
///
/// # Errors
///
/// Each protocol names the error it fails with in [`Error`](Protocol::Error), so its signature
/// lists only the failures it can actually have. The library's protocols use:
///
/// - [`share::Error<T>`](share::Error) for the secret-sharing protocols of [`share`] and
///   [`passive_shamir`]: network failures, failed dealing or reconstruction, and malformed input;
/// - [`zk::Error`] for the proofs of [`zk`]: network failures and malformed input;
/// - [`NetworkError`](crate::net::NetworkError) is enough for a protocol that only exchanges
///   messages.
///
/// A protocol that calls sub-protocols with different error types defines its own error, for
/// example a [`thiserror`](https://docs.rs/thiserror) enum with one `#[from]` variant per
/// sub-protocol error, so that `?` converts each of them. The trait has no default error type,
/// because associated type defaults are not stable Rust.
///
/// The combinators of [`concurrent`] run sub-protocols that share one error type. To run
/// protocols with different error types together, convert their errors into a common type with
/// [`ProtocolExt::err_into`].
pub trait Protocol<E: Environment>: Send + Sync {
    /// The output of the protocol.
    type Output;
    /// The error the protocol fails with. See [Errors](Protocol#errors).
    ///
    /// The bounds let a caller propagate it with `?` into a `Box<dyn std::error::Error + Send +
    /// Sync>` and return it from a task spawned on a multi-threaded runtime.
    type Error: std::error::Error + Send + Sync + 'static;
    /// Behavior of the protocol when run.
    ///
    /// Declared as `fn … -> impl Future + Send` rather than `async fn` so the returned future is
    /// guaranteed [`Send`] and a protocol can be driven on a multi-threaded runtime; implementors
    /// still write a plain `async fn run`.
    fn run(
        self,
        environment: &mut E,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send;
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
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send
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

    /// Erases this protocol's type and maps its output with `map`, so that protocols of different
    /// types can run in one [`JoinAll`](concurrent::JoinAll).
    ///
    /// The result is a [`BoxedProtocol<E, O, Self::Error>`](BoxedProtocol), whose type depends
    /// only on the environment, on the output `O` and on the error type. Map each output into one
    /// type, for example an enum:
    ///
    /// ```ignore
    /// enum Pre { Shares(Vec<Share>), Doubles(Vec<DoubleShare>) }
    ///
    /// let outputs: Vec<Pre> = JoinAll(vec![
    ///     rand_shr.boxed(Pre::Shares),
    ///     rand_double_shr.boxed(Pre::Doubles),
    /// ])
    /// .execute(env)
    /// .await?;
    /// ```
    ///
    /// The boxed protocols of one `JoinAll` must also share an error type. If they don't, convert
    /// each error first with [`err_into`](ProtocolExt::err_into), then box:
    /// `pok.err_into::<MyError>().boxed(Pre::Verdict)`.
    ///
    /// The boxed protocol behaves as this one: it runs the same code on the same environment, and
    /// reports this protocol's [`id`](Protocol::id), so its trace scope carries this protocol's
    /// name. `map` is applied only to a successful output; an error passes through unchanged.
    ///
    /// The protocol and `map` must be `'static`, because they are stored behind a
    /// `Box<dyn …>`; the environment need not be. Boxing costs two heap allocations: one for the
    /// protocol and one for the future that runs it.
    fn boxed<F, O>(self, map: F) -> BoxedProtocol<E, O, Self::Error>
    where
        F: FnOnce(Self::Output) -> O + Send + Sync + 'static,
        Self: Sized + 'static,
    {
        BoxedProtocol::new(self, map)
    }
}

/// Adapters on any protocol, independent of its environment.
///
/// It is implemented for every `Send + Sync` type, so its methods are available on every
/// protocol. Import it, or the [prelude](crate::prelude), to call them.
///
/// # Why a separate trait
///
/// A protocol usually implements [`Protocol<E>`] for every environment `E`. An adapter whose type
/// does not mention `E`, such as [`ErrInto`], therefore cannot be a method of `Protocol<E>`: at
/// the call site nothing determines `E`, and the call fails to compile with a "type annotations
/// needed" error (E0283). This trait has no `E`, so its methods resolve without one, and the
/// adapter's `Protocol<E>` implementation is chosen later, where the adapted protocol is run.
/// ([`Protocol::boxed`] can be a method of `Protocol<E>` because its return type names `E`.)
///
/// # Name clash
///
/// [`futures_util::TryFutureExt`](https://docs.rs/futures-util/latest/futures_util/future/trait.TryFutureExt.html)
/// also has an `err_into` method. With both traits in scope, calling `err_into` on a `Send +
/// Sync` future is ambiguous (E0034); call it as `TryFutureExt::err_into(future)` there.
pub trait ProtocolExt: Sized {
    /// Converts the error of this protocol into `T` with [`Into`], so that it can run alongside
    /// protocols whose error type is `T`.
    ///
    /// The combinators of [`concurrent`] need one error type for all their sub-protocols, and a
    /// [`BoxedProtocol`] fixes its error type. `err_into` adapts a protocol whose error converts
    /// into that type:
    ///
    /// ```rust
    /// use scl_rs::math::field::mersenne61::Mersenne61;
    /// use scl_rs::prelude::*;
    /// use scl_rs::protocol::concurrent::Join;
    /// use scl_rs::protocol::passive_shamir::rand_share::PassiveRandShr;
    /// use scl_rs::protocol::{share, ForkEnvironment};
    ///
    /// /// Sends one packet to every other party and receives one from each.
    /// struct Hello;
    ///
    /// impl<E: Environment> Protocol<E> for Hello {
    ///     type Output = ();
    ///     type Error = NetworkError;
    ///
    ///     async fn run(self, env: &mut E) -> Result<(), NetworkError> {
    ///         let others: Vec<_> = env
    ///             .network()
    ///             .party_ids()
    ///             .into_iter()
    ///             .filter(|p| *p != env.network().local_party())
    ///             .collect();
    ///         for p in &others {
    ///             env.network_mut().send_to(*p, &Packet::empty()).await?;
    ///         }
    ///         for p in &others {
    ///             env.network_mut().recv_from(*p).await?;
    ///         }
    ///         Ok(())
    ///     }
    ///
    ///     fn id(&self) -> ProtocolId {
    ///         ProtocolId::from("Hello")
    ///     }
    /// }
    ///
    /// async fn hello_and_randomness<E: RandEnvironment + ForkEnvironment>(
    ///     env: &mut E,
    ///     t: usize,
    /// ) -> Result<(), share::Error<Mersenne61>> {
    ///     let parties = env.network().party_ids();
    ///     // `Hello` fails with a `NetworkError`, `PassiveRandShr` with a `share::Error`. The
    ///     // network error converts into the share error, so `Hello` is adapted to it.
    ///     let ((), shares) = Join((
    ///         Hello.err_into::<share::Error<Mersenne61>>(),
    ///         PassiveRandShr::<1, Mersenne61>::new(t, parties)?,
    ///     ))
    ///     .execute(env)
    ///     .await?;
    ///     # let _ = shares;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// The adapted protocol runs the same code on the same environment, and reports this
    /// protocol's [`id`](Protocol::id), so the trace shows one scope under this protocol's name.
    /// Name `T` explicitly, with `err_into::<T>()`: nothing at the call site determines it.
    fn err_into<T>(self) -> ErrInto<Self, T> {
        ErrInto {
            protocol: self,
            error: PhantomData,
        }
    }
}

impl<P: Send + Sync> ProtocolExt for P {}

/// A protocol whose error is converted into `T`: what [`ProtocolExt::err_into`] returns.
pub struct ErrInto<P, T> {
    /// The protocol that is run.
    protocol: P,
    /// The target error type. `fn() -> T` keeps `ErrInto` `Send + Sync` whatever `T` is, since no
    /// `T` is stored.
    error: PhantomData<fn() -> T>,
}

impl<E, P, T> Protocol<E> for ErrInto<P, T>
where
    E: Environment,
    P: Protocol<E>,
    P::Error: Into<T>,
    T: std::error::Error + Send + Sync + 'static,
{
    type Output = P::Output;
    type Error = T;

    async fn run(self, environment: &mut E) -> Result<Self::Output, T> {
        // `run`, not `execute`: `ErrInto` is the protocol being executed, and it reports the inner
        // protocol's id, so the inner protocol's scope must not be opened a second time.
        self.protocol.run(environment).await.map_err(Into::into)
    }

    fn id(&self) -> ProtocolId {
        self.protocol.id()
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

/// An [`Environment`] that can be split into child environments, so that independent
/// sub-protocols can run at the same time within one party.
///
/// The combinators in [`concurrent`] require it: they fork the environment once per
/// sub-protocol, and run each sub-protocol on its own child. A child has the same type as its
/// parent. It carries a network handle on a child session (see [`ForkNetwork`]), so the messages
/// of different children never mix, and whatever else the environment holds, split or shared as
/// its implementation chooses.
///
/// [`GeneralEnvironment`] implements it when its network implements [`ForkNetwork`] and its RNG
/// implements [`SeedableRng`].
pub trait ForkEnvironment: Environment {
    /// Returns a child environment, and advances this environment's fork count.
    ///
    /// The child's network handle is on the next child session of this environment's session:
    /// the `i`-th fork, counting from 0, is on child session `i` (see [`ForkNetwork::fork`]).
    ///
    /// Every party must make the same sequence of forks within a session, including a party that
    /// takes no part in some of the sub-protocols; see [Every party forks
    /// alike](crate::net::ForkNetwork#every-party-forks-alike). The combinators fork for you, so
    /// this means every party must run every `Join` and `JoinAll`, with the same number of
    /// sub-protocols.
    ///
    /// Fork through the combinators of [`concurrent`] rather than by calling this method. A
    /// protocol that does call it must do what they do: run nothing else in this environment's
    /// session until every protocol on the child has returned or been dropped. Otherwise a
    /// simulation trace places the child's scope inside the wrong protocol and counts its bytes
    /// under the wrong one; see [Fork only through the
    /// combinators](concurrent#fork-only-through-the-combinators).
    ///
    /// For [`GeneralEnvironment`], the child's RNG is seeded from this environment's RNG. A run
    /// with seeded RNGs therefore stays reproducible, but forking draws from this RNG, so it
    /// changes the values sampled from it afterwards.
    fn fork(&mut self) -> Self;
}

/// Environment that provides the network and the session RNG that traverse through layers.
///
/// This is the most basic environment: it implements both [`Environment`] and
/// [`RandEnvironment`]. If a protocol requires further information that needs to be passed to deep
/// layers of protocol composition, we recommend the API user to create a different struct that
/// implements the [`Environment`] trait.
pub struct GeneralEnvironment<N: Network, R: CryptoRng + Send> {
    /// Network in which the protocol is being executed.
    pub network: N,
    /// Session RNG used by protocols that sample secret material (see [`RandEnvironment`]).
    pub rng: R,
}

impl<N: Network, R: CryptoRng + Send> GeneralEnvironment<N, R> {
    /// Creates a new general environment.
    pub fn new(network: N, rng: R) -> Self {
        Self { network, rng }
    }
}

impl<R, N> Environment for GeneralEnvironment<N, R>
where
    R: CryptoRng + Send,
    N: Network,
{
    type Net = N;

    fn network(&self) -> &Self::Net {
        &self.network
    }

    fn network_mut(&mut self) -> &mut Self::Net {
        &mut self.network
    }
}

impl<R, N> ForkEnvironment for GeneralEnvironment<N, R>
where
    R: CryptoRng + SeedableRng + Send,
    N: ForkNetwork,
{
    fn fork(&mut self) -> Self {
        Self::new(self.network.fork(), R::from_rng(&mut self.rng))
    }
}

impl<R: CryptoRng + Send, N: Network> RandEnvironment for GeneralEnvironment<N, R> {
    type Rng = R;
    fn rng_mut(&mut self) -> &mut Self::Rng {
        &mut self.rng
    }
}
