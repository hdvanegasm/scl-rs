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
/// Two variants ([`CloseChannel`](Event::CloseChannel) and [`HasData`](Event::HasData)) are
/// retained for compatibility but are **not** produced by the current deterministic core: the
/// event-loop model has no persistent channels to close and no `has_data` polling.
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
    },
    /// The party received a packet from a peer.
    ReceiveData {
        /// Receiver's virtual time when the packet was delivered.
        timestamp: Duration,
        /// The directed link the packet was received on (`sender` → `recipient`).
        link: Link,
        /// Size of the packet payload, in bytes.
        size: usize,
    },
    /// A peer's channel has data ready to be received.
    ///
    /// Legacy event, not produced by the current event-loop core (there is no `has_data` polling).
    HasData {
        /// Virtual time of the observation.
        timestamp: Duration,
        /// The directed link that has data available.
        link: Link,
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
            | Event::HasData { timestamp, .. }
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
            Event::HasData { .. } => EventType::HasData,
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

        let (name, details) = match self {
            Event::Start { .. } => ("START", String::new()),
            Event::Stop { .. } => ("STOP", String::new()),
            Event::Killed { reason, .. } => ("KILLED", format!("reason: {reason}")),
            Event::Cancelled { .. } => ("CANCELLED", String::new()),
            Event::CloseChannel { link, .. } => ("CLOSE_CHANNEL", outgoing(link)),
            Event::SendData { link, size, .. } => {
                ("SEND", format!("{} ({size} bytes)", outgoing(link)))
            }
            Event::ReceiveData { link, size, .. } => {
                ("RECV", format!("{} ({size} bytes)", incoming(link)))
            }
            Event::HasData { link, .. } => ("HAS_DATA", incoming(link)),
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
    /// See [`Event::HasData`].
    HasData,
    /// See [`Event::Sleep`].
    Sleep,
    /// See [`Event::Output`].
    Output,
    /// See [`Event::ProtocolBegin`].
    ProtocolBegin,
    /// See [`Event::ProtocolEnd`].
    ProtocolEnd,
}
