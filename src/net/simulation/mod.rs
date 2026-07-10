//! Deterministic, single-threaded simulator for MPC protocols.
//!
//! A protocol is written *once*, generic over the [`Network`](crate::net::Network) trait, and runs
//! unchanged on either a real TCP/TLS network ([`TcpNetwork`](crate::net::TcpNetwork)) or this
//! simulator ([`SimNetwork`](crate::net::simulation::network::SimNetwork)). The simulator needs no sockets, threads, or
//! wall-clock timing: it models the network analytically and advances a *virtual* clock, so a run
//! is fully reproducible and reports timings close to a real deployment with the same network
//! parameters.
//!
//! # How it works
//!
//! Instead of running each party on an async runtime such as tokio, the simulator *is* the
//! executor. A party's `recv_from` future, when its message has not arrived yet, registers that
//! the party is blocked and returns `Poll::Pending`. The executor then knows exactly who is
//! waiting on whom, advances the virtual clock to the next deliverable message, delivers it, and
//! re-polls. This gives explicit blocking state (as in a sans-IO design) while protocols keep
//! ordinary straight-line `async`/`await` code.
//!
//! The single rule that makes a protocol portable across both executors: it may only suspend
//! (`.await`) through abstractions both executors implement — in practice the
//! [`Network`](crate::net::Network) trait. Suspending through a tokio-only primitive (a raw
//! `tokio::time::sleep`, `tokio::spawn`, a background thread) would have nothing to wake it under
//! the deterministic executor and breaks reproducibility.
//!
//! # Module layout
//!
//! Dependencies point one way (`simulator` → {`executor`, `switchboard`, `network`}; `executor` and
//! `switchboard` are independent):
//!
//! - [`executor`](crate::net::simulation::executor) — the network-agnostic scheduler: a dumb pump
//!   that polls ready party tasks and, when all are parked, asks an idle handler to make progress.
//! - [`switchboard`](crate::net::simulation::switchboard) — the in-memory message router and
//!   virtual-time event loop ([`Switchboard`](crate::net::simulation::switchboard::Switchboard), the
//!   [`Delay`](crate::net::simulation::switchboard::Delay) timing model), plus trace recording and
//!   hook dispatch.
//! - [`hook`](crate::net::simulation::hook) — the
//!   [`TriggeredHook`](crate::net::simulation::hook::TriggeredHook) extension point, which fires on
//!   recorded events, and the built-in [`MetricHook`](crate::net::simulation::hook::MetricHook).
//! - [`network`](crate::net::simulation::network) —
//!   [`SimNetwork`](crate::net::simulation::network::SimNetwork), the simulated
//!   [`Network`](crate::net::Network) implementation.
//! - [`simulator`](crate::net::simulation::simulator) — the top-level
//!   [`simulate`](crate::net::simulation::simulator::simulate) driver, returning a
//!   [`SimulationOutcome`](crate::net::simulation::simulator::SimulationOutcome) of per-party outputs
//!   and traces.
//! - [`channel`](crate::net::simulation::channel) — shared channel configuration
//!   ([`ChannelConfig`](crate::net::simulation::channel::ChannelConfig),
//!   [`NetworkConfig`](crate::net::simulation::channel::NetworkConfig)), the directed
//!   [`Link`](crate::net::simulation::channel::Link), and the network timing math.
//! - [`event`](crate::net::simulation::event) — the
//!   [`Event`](crate::net::simulation::event::Event) records collected into a
//!   [`SimulationTrace`](crate::net::simulation::SimulationTrace).
//!
//! # Example
//!
//! Run a two-party protocol on the simulator and read back each party's output:
//!
//! ```ignore
//! use scl_rs::net::simulation::channel::SimpleNetworkConfig;
//! use scl_rs::net::simulation::simulator::simulate;
//! use scl_rs::net::PartyId;
//! use scl_rs::protocol::GeneralEnv;
//!
//! let parties = vec![PartyId::from(0), PartyId::from(1)];
//! let outcome = simulate(
//!     SimpleNetworkConfig,
//!     parties,
//!     |_| SendRecvProtocol,
//!     |_, net| GeneralEnv::new(net),
//!     vec![],
//! );
//! let output_p0 = &outcome.outputs[&PartyId::from(0)];
//! ```

use crate::net::simulation::channel::{ChannelConfigBuilder, Link};
use crate::net::simulation::event::{Event, EventType};
use crate::net::{NetworkError, PartyId};
use thiserror::Error;
use tokio::task::JoinError;

/// Implementation of an executor for protocols.
pub mod executor;

/// Hooks that fire on recorded simulation events.
pub mod hook;

/// Top-level simulation driver: [`simulator::simulate`] runs every party's protocol on the
/// deterministic core and returns their outputs and event traces.
pub mod simulator;

/// Switch board implementation.
pub mod switchboard;

/// Channel configuration and the network timing model.
pub mod channel;

/// Simulation events.
pub mod event;

/// Implement the network simulation.
pub mod network;

/// Errors for protocol simulations.
#[derive(Error, Debug)]
#[non_exhaustive]
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
    #[error("channel {link:?} not found: {err_context}")]
    ChannelNotFound {
        /// The missing channel's directed link.
        link: Link,
        /// Context for the error.
        err_context: &'static str,
    },
    /// The set of sent messages is empty.
    #[error("sends are empty")]
    SendsEmpty,
    /// Invalid parameters for a channel configuration.
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

    /// Checks if the trace is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
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

/// Renders the whole trace as an indented tree, in the order in which the events occurred.
///
/// Each protocol scope is delimited by a [`ProtocolBegin`](Event::ProtocolBegin) /
/// [`ProtocolEnd`](Event::ProtocolEnd) pair (recorded by
/// [`Protocol::execute`](crate::protocol::Protocol::execute)), rendered as a brace block: a
/// protocol-begin line prints `<name> {` and indents everything that follows it; its matching
/// protocol-end line prints `}` at the same level as the opening line. Because protocols nest when
/// one calls another, the indentation shows the composition structure:
///
/// ```text
/// SecureAdditionAdditiveShare {
///     DistributeAdditiveShare {
///         [     0.000s] SEND           0 -> 0 (33 bytes)
///         ...
///     }
///     ReconstructAdditiveShare {
///         ...
///     }
/// }
/// ```
///
/// Every other event keeps its single-line `[<timestamp>s] <EVENT> <details>` form (see the
/// [`Event`] display). This is meant for debugging a protocol's behavior: it can be printed with
/// `println!("{trace}")` or written to a file with `write!(file, "{trace}")`.
impl std::fmt::Display for SimulationTrace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const INDENT: &str = "    ";
        let mut depth: usize = 0;
        for (idx, event) in self.0.iter().enumerate() {
            if idx > 0 {
                writeln!(f)?;
            }
            // A closing brace dedents *before* it is printed, so it aligns with the line that
            // opened the block. `saturating_sub` keeps rendering robust if a trace is unbalanced.
            if let Event::ProtocolEnd { .. } = event {
                depth = depth.saturating_sub(1);
            }
            for _ in 0..depth {
                f.write_str(INDENT)?;
            }
            match event {
                Event::ProtocolBegin { protocol_name, .. } => {
                    write!(f, "{protocol_name} {{")?;
                    // Everything up to the matching closing brace is nested one level deeper.
                    depth += 1;
                }
                Event::ProtocolEnd { .. } => write!(f, "}}")?,
                other => write!(f, "{other}")?,
            }
        }
        Ok(())
    }
}
