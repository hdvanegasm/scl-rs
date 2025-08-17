use crate::net::simulation::channel::ChannelId;
use std::time::Duration;

pub enum Event {
    Simulation(SimulationEvent),
    Channel(ChannelEvent),
    Protocol(ProtocolEvent),
}

/// Type of the event.
pub enum SimulationEvent {
    Start,
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

pub enum ChannelEvent {}

pub enum ProtocolEvent {}

pub struct SimulationTrace(Vec<Event>);

impl SimulationTrace {
    fn empty() -> Self {
        SimulationTrace(Vec::new())
    }

    fn add_event(&mut self, event: Event) {
        self.0.push(event);
    }
}
