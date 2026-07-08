//! Trace records describing what happened during a simulation.
//!
//! An [`Event`] captures a single moment in a party's run — start/stop, a send or receive, a
//! protocol boundary, a produced output — stamped with the party's *virtual* time. Events are
//! appended to a [`SimulationTrace`](crate::net::simulation::SimulationTrace) as the protocol runs,
//! and each renders to a compact human-readable line through its [`Display`](std::fmt::Display) impl.
//! [`EventType`] is the data-less discriminant, used to filter traces and to declare which event a
//! [`TriggeredHook`](crate::net::simulation::switchboard::TriggeredHook) reacts to.

use crate::net::simulation::channel::Link;
use std::fmt;
use std::time::Duration;

/// An event recorded during the simulation, carrying its timestamp and any associated data.
///
/// Events are appended to a [`SimulationTrace`](crate::net::simulation::SimulationTrace) as a
/// protocol runs, giving a per-party, time-ordered record of what happened. Every timestamp is the
/// recording party's *virtual* time (see the network timing model in
/// [`channel`](crate::net::simulation::channel)), not wall-clock time.
///
/// One variant ([`CloseChannel`](Event::CloseChannel)) is retained for compatibility but is
/// **not** produced by the current deterministic core: the event-loop model has no persistent
/// channels to close.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The party started its execution, before running any protocol.
    Start {
        /// Virtual time at which the party started.
        timestamp: Duration,
    },
    /// The party finished its execution, after its protocol completed.
    Stop {
        /// Virtual time at which the party stopped.
        timestamp: Duration,
    },
    /// The party's execution was forcibly terminated.
    Killed {
        /// Virtual time at which the party was killed.
        timestamp: Duration,
        /// Human-readable reason for the termination.
        reason: String,
    },
    /// The party's execution was cancelled.
    Cancelled {
        /// Virtual time at which the party was cancelled.
        timestamp: Duration,
    },
    /// A channel was closed.
    ///
    /// Legacy event, not produced by the current event-loop core (there are no persistent
    /// channels to close).
    CloseChannel {
        /// Virtual time at which the channel was closed.
        timestamp: Duration,
        /// The directed link that was closed.
        link: Link,
    },
    /// The party sent a packet to a peer.
    SendData {
        /// Sender's virtual time when the packet was handed to the network.
        timestamp: Duration,
        /// The directed link the packet was sent on (`sender` → `recipient`).
        link: Link,
        /// Size of the packet payload, in bytes.
        size: usize,
        /// Per-type breakdown of the packet's elements as `(label, count)` pairs, in first-appearance
        /// order (from [`Packet::composition`](crate::net::Packet::composition)). Elements written
        /// without a type label are grouped under `unknown elem.`. Rendered after the byte size in
        /// the `SEND` line, e.g. `(1024 bytes: 1 EC elem., 4 field elem.)`.
        content_count: Vec<(&'static str, usize)>,
    },
    /// The party received a packet from a peer.
    ReceiveData {
        /// Receiver's virtual time when the packet was delivered.
        timestamp: Duration,
        /// The directed link the packet was received on (`sender` → `recipient`).
        link: Link,
        /// Size of the packet payload, in bytes.
        size: usize,
        /// Per-type breakdown of the packet's elements as `(label, count)` pairs, in first-appearance
        /// order (from [`Packet::composition`](crate::net::Packet::composition)). These are the
        /// **sender's** labels, carried in-process by the simulator (which does not serialize
        /// packets), not what the receiver will deserialize the elements into. Elements written
        /// without a type label are grouped under `unknown elem.`. Rendered after the byte size in
        /// the `RECV` line, e.g. `(1024 bytes: 1 EC elem., 4 field elem.)`.
        content_count: Vec<(&'static str, usize)>,
    },
    /// The party slept (waited) for a fixed duration.
    Sleep {
        /// Virtual time at which the sleep began.
        timestamp: Duration,
        /// How long the party slept.
        duration: Duration,
    },
    /// The party emitted an output (its protocol's typed output, serialized to bytes for the trace).
    Output {
        /// Virtual time at which the output was produced.
        timestamp: Duration,
        /// The serialized output bytes.
        output: Vec<u8>,
    },
    /// A protocol began running.
    ProtocolBegin {
        /// Virtual time at which the protocol started.
        timestamp: Duration,
        /// Name of the protocol, as reported by [`Protocol::name`](crate::protocol::Protocol::name).
        protocol_name: &'static str,
    },
    /// A protocol finished running.
    ProtocolEnd {
        /// Virtual time at which the protocol finished.
        timestamp: Duration,
        /// Name of the protocol, as reported by [`Protocol::name`](crate::protocol::Protocol::name).
        protocol_name: &'static str,
    },
}

impl Event {
    /// Returns the virtual timestamp at which this event occurred.
    pub fn timestamp(&self) -> Duration {
        match self {
            Event::Start { timestamp, .. }
            | Event::Stop { timestamp, .. }
            | Event::Killed { timestamp, .. }
            | Event::Cancelled { timestamp }
            | Event::CloseChannel { timestamp, .. }
            | Event::SendData { timestamp, .. }
            | Event::ReceiveData { timestamp, .. }
            | Event::Sleep { timestamp, .. }
            | Event::Output { timestamp, .. }
            | Event::ProtocolBegin { timestamp, .. }
            | Event::ProtocolEnd { timestamp, .. } => *timestamp,
        }
    }

    /// Returns the [`EventType`] discriminant of this event, dropping its associated data.
    ///
    /// Useful for matching a [`TriggeredHook`](crate::net::simulation::switchboard::TriggeredHook)
    /// trigger or asserting on the shape of a trace without caring about payloads.
    pub fn event_type(&self) -> EventType {
        match self {
            Event::Start { .. } => EventType::Start,
            Event::Stop { .. } => EventType::Stop,
            Event::Killed { .. } => EventType::Killed,
            Event::Cancelled { .. } => EventType::Cancelled,
            Event::CloseChannel { .. } => EventType::CloseChannel,
            Event::SendData { .. } => EventType::SendData,
            Event::ReceiveData { .. } => EventType::ReceiveData,
            Event::Sleep { .. } => EventType::Sleep,
            Event::Output { .. } => EventType::Output,
            Event::ProtocolBegin { .. } => EventType::ProtocolBegin,
            Event::ProtocolEnd { .. } => EventType::ProtocolEnd,
        }
    }
}

/// Renders an event as a single human-readable line, useful for debugging a protocol's behavior.
///
/// The format is `[<timestamp>s] <EVENT_NAME> <details>`. Links are shown from the recording
/// party's perspective: `sender -> recipient` for outgoing operations and `recipient <- sender`
/// for incoming ones, so the recording party is always on the left.
///
/// `SEND` and `RECV` lines additionally report the packet's per-type element breakdown after the
/// byte size, e.g. `SEND  2 -> 0 (1024 bytes: 1 EC elem., 4 field elem.)`; elements written without
/// a type label show up as `unknown elem.` (see
/// [`Packet::write_labeled`](crate::net::Packet::write_labeled)). On `RECV` these are the sender's
/// labels, carried in-process by the simulator.
impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The recording party is always shown on the left of the arrow, so the link reads from its
        // own perspective: an outgoing operation is `sender -> recipient` and an incoming one is
        // `recipient <- sender`.
        let outgoing = |link: &Link| {
            format!(
                "{} -> {}",
                link.sender().as_usize(),
                link.recipient().as_usize()
            )
        };
        let incoming = |link: &Link| {
            format!(
                "{} <- {}",
                link.recipient().as_usize(),
                link.sender().as_usize()
            )
        };

        // Renders the byte size followed by the per-type element breakdown, e.g.
        // `1024 bytes: 1 EC elem., 4 field elem.`. Shared by the `SEND` and `RECV` lines.
        let breakdown = |size: usize, content_count: &[(&'static str, usize)]| {
            let mut count_content = String::new();
            for (label, count) in content_count {
                count_content += &format!("{count} {label}, ");
            }
            format!("{size} bytes: {}", count_content.trim_end_matches(", "))
        };

        let (name, details) = match self {
            Event::Start { .. } => ("START", String::new()),
            Event::Stop { .. } => ("STOP", String::new()),
            Event::Killed { reason, .. } => ("KILLED", format!("reason: {reason}")),
            Event::Cancelled { .. } => ("CANCELLED", String::new()),
            Event::CloseChannel { link, .. } => ("CLOSE_CHANNEL", outgoing(link)),
            Event::SendData {
                link,
                size,
                content_count,
                ..
            } => (
                "SEND",
                format!("{} ({})", outgoing(link), breakdown(*size, content_count)),
            ),
            Event::ReceiveData {
                link,
                size,
                content_count,
                ..
            } => (
                "RECV",
                format!("{} ({})", incoming(link), breakdown(*size, content_count)),
            ),
            Event::Sleep { duration, .. } => ("SLEEP", format!("for {duration:?}")),
            Event::Output { output, .. } => {
                // Show small payloads in full; for larger ones show the first and last few bytes
                // along with the total length to keep the trace readable.
                const HEAD: usize = 4;
                const TAIL: usize = 4;
                let details = if output.len() <= HEAD + TAIL {
                    format!("{output:?}")
                } else {
                    let join = |bytes: &[u8]| {
                        bytes
                            .iter()
                            .map(|byte| byte.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    format!(
                        "[{}, …, {}] ({} bytes)",
                        join(&output[..HEAD]),
                        join(&output[output.len() - TAIL..]),
                        output.len()
                    )
                };
                ("OUTPUT", details)
            }
            Event::ProtocolBegin { protocol_name, .. } => {
                ("PROTOCOL_BEGIN", String::from(*protocol_name))
            }
            Event::ProtocolEnd { protocol_name, .. } => {
                ("PROTOCOL_END", String::from(*protocol_name))
            }
        };

        let timestamp = self.timestamp().as_secs_f64();
        if details.is_empty() {
            write!(f, "[{timestamp:>10.3}s] {name}")
        } else {
            write!(f, "[{timestamp:>10.3}s] {name:<14} {details}")
        }
    }
}

/// The kind of an [`Event`], without its associated data.
///
/// Obtained via [`Event::event_type`]; used to filter events and to declare which event a
/// [`TriggeredHook`](crate::net::simulation::switchboard::TriggeredHook) reacts to.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum EventType {
    /// See [`Event::Start`].
    Start,
    /// See [`Event::Stop`].
    Stop,
    /// See [`Event::Killed`].
    Killed,
    /// See [`Event::Cancelled`].
    Cancelled,
    /// See [`Event::CloseChannel`].
    CloseChannel,
    /// See [`Event::SendData`].
    SendData,
    /// See [`Event::ReceiveData`].
    ReceiveData,
    /// See [`Event::Sleep`].
    Sleep,
    /// See [`Event::Output`].
    Output,
    /// See [`Event::ProtocolBegin`].
    ProtocolBegin,
    /// See [`Event::ProtocolEnd`].
    ProtocolEnd,
}
