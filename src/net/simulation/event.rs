use crate::net::simulation::channel::ChannelId;
use std::time::Duration;

/// Type of the event.
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

pub struct SimulationTrace(Vec<Event>);

impl SimulationTrace {
    pub fn empty() -> Self {
        SimulationTrace(Vec::new())
    }

    pub fn add_event(&mut self, event: Event) {
        self.0.push(event);
    }
}
