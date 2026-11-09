//!
//! A protocol calls a sub-protocol with [`execute`](Protocol::execute), which borrows the
//! environment mutably until the sub-protocol returns, so sub-protocols called that way run one
//! after another. When they are independent, a party then waits for the sum of their latencies
//! instead of for the slowest one. The combinators in this module run them concurrently instead:
//!
//! - [`Join`](crate::protocol::concurrent::Join) runs a fixed number of protocols, of possibly
//!   different types, and outputs the tuple of their outputs.
//! - [`JoinAll`](crate::protocol::concurrent::JoinAll) runs any number of protocols of one type,
//!   and outputs a `Vec` of their outputs in input order. To put protocols of different types in
//!   one `JoinAll`, box each with [`Protocol::boxed`](crate::protocol::Protocol::boxed), which maps
//!   its output into a common type.
//!
//! Both are ordinary [`Protocol`]s: they nest, so a `Join` may contain another `Join`, and they are
//! called with `execute` like any other protocol. Their environment must implement
//! [`ForkEnvironment`]. A combinator forks the environment once per sub-protocol, and each
//! sub-protocol runs on its own child environment, with a network handle on its own child session
//! (see [`ForkNetwork`](crate::net::ForkNetwork)) and its own RNG. Messages of different
//! sub-protocols therefore never mix, even over the same links.
//!
//! The concurrency stays within the calling party's task: no task or thread is spawned. A
//! combinator therefore adds no CPU parallelism, and runs deterministically on the simulator like
//! any other protocol.
//!
//! # Every party runs every branch
//!
//! Child sessions are numbered by the order of the forks, with no communication, so the parties
//! agree on them only if they fork alike (see [Every party forks
//! alike](crate::net::ForkNetwork#every-party-forks-alike)). **Every party must run every `Join` and
//! `JoinAll`, with the same number of sub-protocols**, including a party that takes no part in
//! some of them. In that position, such a party runs a sub-protocol that returns at once: for
//! instance, a wrapper whose output is an `Option` that is `None` on the parties that take no part.
//!
//! Do not use a protocol that rejects non-participants as that sub-protocol.
//! [`SchnorrPok`](crate::protocol::zk::schnorr::SchnorrPok), for example, returns
//! [`Error::Input`] on a party that is neither the prover nor the verifier, and that error aborts
//! the whole combinator (see [Errors](#errors)).
//!
//! If a party skips a combinator, every later fork in that session gets a different index on that
//! party, so the problem shows up at a later combinator, not necessarily at the skipped one. On the
//! simulator, it ends in a deadlock panic that lists the packets left in sessions their recipient
//! never opened. On a real network, the receives hang until they time out.
//!
//! # Errors
//!
//! A combinator fails fast: it returns the first error any of its sub-protocols returns, and drops
//! the others where they are, unfinished. Then:
//!
//! - the messages they already sent are never received;
//! - at the other parties, the matching sub-protocols may wait for messages that never come, until
//!   their own timeout or error;
//! - their trace scopes stay open, since they never reach their end.
//!
//! The calling party's own session is not affected, so the caller may handle the error and keep
//! using its environment.
//!
//! # Randomness
//!
//! With [`GeneralEnv`](crate::protocol::GeneralEnv), each child's RNG is seeded from the parent's
//! RNG when the child is forked, so a run with seeded RNGs stays reproducible. Forking draws from
//! the parent's RNG, though, so running sub-protocols under a combinator instead of one after
//! another changes the random values they sample. A test that compares the two forms should
//! compare what the outputs open to (for example, that a product opens to the product of its
//! factors), not the shares themselves.
//!
//! # Batch or join
//!
//! When a protocol is vectorized, meaning one instance handles a vector of inputs in the same
//! rounds, as [`PassiveShamirMul`](crate::protocol::passive_shamir::mul::PassiveShamirMul) does,
//! prefer one instance over all the inputs to a `JoinAll` of one instance per input. Both take the
//! same number of rounds, but the single instance sends one message per link and round, where the
//! `JoinAll` sends one per instance. Use `Join` when the sub-protocols have different types or
//! round structures, and `JoinAll` when the protocol cannot be batched.
//!
//! # Example
//!
//! Preprocessing for multiplication needs random sharings and double sharings. Generating them is
//! independent, so the three protocols below run at the same time, and the party waits for the
//! slowest of them instead of for all three in turn:
//!
//! ```rust
//! use scl_rs::math::field::mersenne61::Mersenne61;
//! use scl_rs::prelude::*;
//! use scl_rs::protocol::concurrent::Join;
//! use scl_rs::protocol::passive_shamir::{
//!     double_rand_share::PassiveRandDoubleShr, rand_share::PassiveRandShr,
//! };
//! use scl_rs::protocol::ForkEnvironment;
//!
//! async fn preprocess<E: RandEnvironment + ForkEnvironment>(
//!     env: &mut E,
//!     t: usize,
//! ) -> Result<(), Error> {
//!     let parties = env.network().party_ids();
//!     let (a, b, doubles) = Join((
//!         PassiveRandShr::<1, Mersenne61>::new(t, parties.clone())?,
//!         PassiveRandShr::<1, Mersenne61>::new(t, parties.clone())?,
//!         PassiveRandDoubleShr::<1, Mersenne61>::new(t, parties)?,
//!     ))
//!     .execute(env)
//!     .await?;
//!     // `a` and `b` are each a `Vec` of random sharings, `doubles` a `Vec` of double sharings.
//!     # let _ = (a, b, doubles);
//!     Ok(())
//! }
//! ```

use std::future::Future;

use futures_util::future::BoxFuture;

use crate::protocol::{Environment, Error, ForkEnvironment, Protocol, ProtocolId};

/// Runs a fixed number of protocols, of possibly different types, concurrently, and outputs the
/// tuple of their outputs.
///
/// `Join((p0, p1, …))` is itself a [`Protocol`]. Running it forks the environment once per
/// protocol, in tuple order and before any of them starts. It then runs each protocol with
/// [`execute`](Protocol::execute) on its own child environment, so that in a simulation trace each
/// protocol appears under its own name and in its own session. It returns once all of them have
/// finished, with the outputs in tuple order, whatever order the protocols finish in.
///
/// It is implemented for tuples of 2 to 6 protocols. For more, nest `Join`s, or use [`JoinAll`]
/// when the protocols have one type.
///
/// Every party must run the same `Join`, with the same number of protocols; see [Every party runs
/// every branch](self#every-party-runs-every-branch).
///
/// # Errors
///
/// Returns the first error any of the protocols returns, and drops the others unfinished; see
/// [Errors](self#errors).
pub struct Join<T>(pub T);

// Implements `Protocol` for `Join` over a tuple of one arity. Each argument describes one position
// of the tuple: the protocol's type parameter, the binding that takes the protocol out of the
// tuple, and the binding of its forked environment, for example `P0 p0 e0`. The bindings are passed
// in because `macro_rules!` cannot build a distinct identifier per repetition.
macro_rules! impl_protocol_for_join {
    ($($P:ident $p:ident $e:ident), +) => {
        impl<E: ForkEnvironment, $($P: Protocol<E>),+> Protocol<E> for Join<($($P,)+)>
        where $($P::Output: Send,)+
        {
            type Output = ($($P::Output,)+);

            async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
                let Join(($($p,)+)) = self;
                // Fork every child before running any, in tuple order: child sessions are numbered
                // by the order of the forks, so every party must fork in the same order.
                $(let mut $e = env.fork();)+
                futures_util::try_join!($($p.execute(&mut $e)),+)
            }

            fn id(&self) -> ProtocolId { ProtocolId::from("Join") }
        }
    };
}

impl_protocol_for_join!(P0 p0 e0, P1 p1 e1);
impl_protocol_for_join!(P0 p0 e0, P1 p1 e1, P2 p2 e2);
impl_protocol_for_join!(P0 p0 e0, P1 p1 e1, P2 p2 e2, P3 p3 e3);
impl_protocol_for_join!(P0 p0 e0, P1 p1 e1, P2 p2 e2, P3 p3 e3, P4 p4 e4);
impl_protocol_for_join!(P0 p0 e0, P1 p1 e1, P2 p2 e2, P3 p3 e3, P4 p4 e4, P5 p5 e5);

/// Runs any number of protocols of one type concurrently, and outputs a `Vec` of their outputs in
/// input order.
///
/// `JoinAll(vec![p0, p1, …])` is itself a [`Protocol`]. Running it forks the environment once per
/// element, in input order and before any of them starts. It then runs each protocol on its own
/// child environment, and returns once all of them have finished, with the outputs in input order,
/// whatever order the protocols finish in. An empty `Vec` forks nothing and outputs an empty `Vec`.
///
/// Every party must run the same `JoinAll`, with a `Vec` of the same length: the number of forks
/// must not depend on the party. See [Every party runs every
/// branch](self#every-party-runs-every-branch).
///
/// For a protocol that takes a vector of inputs, one instance over all the inputs sends fewer
/// messages than a `JoinAll` of one instance per input; see [Batch or join](self#batch-or-join).
///
/// # Errors
///
/// Returns the first error any of the protocols returns, and drops the others unfinished; see
/// [Errors](self#errors).
pub struct JoinAll<P>(pub Vec<P>);

impl<E, P> Protocol<E> for JoinAll<P>
where
    E: ForkEnvironment,
    P: Protocol<E>,
    P::Output: Send,
{
    type Output = Vec<P::Output>;

    async fn run(self, environment: &mut E) -> Result<Self::Output, Error> {
        // Fork every child before running any, in input order: child sessions are numbered by the
        // order of the forks, so every party must fork in the same order.
        let mut envs: Vec<_> = self.0.iter().map(|_| environment.fork()).collect();
        futures_util::future::try_join_all(
            self.0
                .into_iter()
                .zip(envs.iter_mut())
                .map(|(p, env)| p.run(env)),
        )
        .await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("JoinAll")
    }
}

/// The form of [`Protocol`] that can be used as a trait object, behind [`BoxedProtocol`].
///
/// `Protocol` itself cannot: its `run` takes `self` by value and returns `impl Future`, whose type
/// differs per implementation. Here the protocol is consumed from a `Box`, and the future is boxed
/// into one type, [`BoxFuture`].
trait DynProtocol<E: Environment, O>: Send + Sync {
    /// Runs the boxed protocol, as [`Protocol::run`] does.
    fn run_boxed<'a>(self: Box<Self>, env: &'a mut E) -> BoxFuture<'a, Result<O, Error>>;
    /// The boxed protocol's id, as [`Protocol::id`] returns it.
    fn id(&self) -> ProtocolId;
}

/// A protocol together with the map applied to its output: the type that [`Protocol::boxed`]
/// boxes.
struct MappedProtocol<P, F> {
    protocol: P,
    map: F,
}

impl<E, O, P, F> DynProtocol<E, O> for MappedProtocol<P, F>
where
    E: Environment,
    P: Protocol<E> + 'static,
    F: FnOnce(P::Output) -> O + Send + Sync + 'static,
{
    fn run_boxed<'a>(self: Box<Self>, env: &'a mut E) -> BoxFuture<'a, Result<O, Error>> {
        let MappedProtocol { protocol, map } = *self;
        Box::pin(async move { protocol.run(env).await.map(map) })
    }

    fn id(&self) -> ProtocolId {
        self.protocol.id()
    }
}

/// A protocol whose type is erased, with output `O`: what [`Protocol::boxed`] returns.
///
/// Protocols of different types become the same type once boxed with the same output type, so
/// they can run in one [`JoinAll`]. A boxed protocol runs the same code as the protocol it was
/// made from, and reports that protocol's [`id`](Protocol::id). See [`Protocol::boxed`].
pub struct BoxedProtocol<E, O>(Box<dyn DynProtocol<E, O>>);

impl<E, O> BoxedProtocol<E, O>
where
    E: Environment,
{
    /// Boxes `protocol` with `map` applied to its output. Called through [`Protocol::boxed`].
    pub(crate) fn new<P, F>(protocol: P, map: F) -> Self
    where
        P: Protocol<E> + 'static,
        F: FnOnce(P::Output) -> O + Send + Sync + 'static,
    {
        BoxedProtocol(Box::new(MappedProtocol { protocol, map }))
    }
}

impl<E, O> Protocol<E> for BoxedProtocol<E, O>
where
    E: Environment,
{
    type Output = O;

    fn run(self, environment: &mut E) -> impl Future<Output = Result<Self::Output, Error>> + Send {
        self.0.run_boxed(environment)
    }

    fn id(&self) -> ProtocolId {
        self.0.id()
    }
}
