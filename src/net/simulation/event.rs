use crate::net::simulation::channel::ChannelId;
use std::fmt;
use std::time::Duration;

/// An event recorded during the simulation, carrying its timestamp and any associated data.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Start {
        timestamp: Duration,
    },
    Stop {
        timestamp: Duration,
    },
    Killed {
        timestamp: Duration,
        reason: String,
    },
    Cancelled {
        timestamp: Duration,
    },
    CloseChannel {
        timestamp: Duration,
        channel_id: ChannelId,
    },
    SendData {
        timestamp: Duration,
        channel_id: ChannelId,
        size: usize,
    },
    ReceiveData {
        timestamp: Duration,
        channel_id: ChannelId,
        size: usize,
    },
    HasData {
        timestamp: Duration,
        channel_id: ChannelId,
    },
    Sleep {
        timestamp: Duration,
        duration: Duration,
    },
    Output {
        timestamp: Duration,
        output: Vec<u8>,
    },
    ProtocolBegin {
        timestamp: Duration,
        protocol_name: String,
    },
    ProtocolEnd {
        timestamp: Duration,
        protocol_name: String,
    },
}

impl Event {
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
/// The format is `[<timestamp>s] <EVENT_NAME> <details>`, where channels are shown as
/// `local -> remote` for outgoing operations and `local <- remote` for incoming ones.
impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let channel = |id: &ChannelId, arrow: &str| {
            format!(
                "{} {arrow} {}",
                id.local().as_usize(),
                id.remote().as_usize()
            )
        };

        let (name, details) = match self {
            Event::Start { .. } => ("START", String::new()),
            Event::Stop { .. } => ("STOP", String::new()),
            Event::Killed { reason, .. } => ("KILLED", format!("reason: {reason}")),
            Event::Cancelled { .. } => ("CANCELLED", String::new()),
            Event::CloseChannel { channel_id, .. } => ("CLOSE_CHANNEL", channel(channel_id, "->")),
            Event::SendData {
                channel_id, size, ..
            } => (
                "SEND",
                format!("{} ({size} bytes)", channel(channel_id, "->")),
            ),
            Event::ReceiveData {
                channel_id, size, ..
            } => (
                "RECV",
                format!("{} ({size} bytes)", channel(channel_id, "<-")),
            ),
            Event::HasData { channel_id, .. } => ("HAS_DATA", channel(channel_id, "<-")),
            Event::Sleep { duration, .. } => ("SLEEP", format!("for {duration:?}")),
            Event::Output { output, .. } => {
                // Show small payloads in full; for larger ones show only the first few bytes plus
                // a count of the remaining ones to keep the trace readable.
                const HEAD: usize = 8;
                let details = if output.len() <= HEAD {
                    format!("{output:?}")
                } else {
                    format!(
                        "{:?} … (+{} more bytes)",
                        &output[..HEAD],
                        output.len() - HEAD
                    )
                };
                ("OUTPUT", details)
            }
            Event::ProtocolBegin { protocol_name, .. } => ("PROTOCOL_BEGIN", protocol_name.clone()),
            Event::ProtocolEnd { protocol_name, .. } => ("PROTOCOL_END", protocol_name.clone()),
        };

        let timestamp = self.timestamp().as_secs_f64();
        if details.is_empty() {
            write!(f, "[{timestamp:>13.6}s] {name}")
        } else {
            write!(f, "[{timestamp:>13.6}s] {name:<14} {details}")
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum EventType {
    Start,
    Stop,
    Killed,
    Cancelled,
    CloseChannel,
    SendData,
    ReceiveData,
    HasData,
    Sleep,
    Output,
    ProtocolBegin,
    ProtocolEnd,
}
