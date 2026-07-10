//! Hooks that observe (or steer) a simulation as its events are recorded.
//!
//! A hook implements [`TriggeredHook`](crate::net::simulation::hook::TriggeredHook) and is
//! registered by passing it to [`simulate`](crate::net::simulation::simulator::simulate). Every
//! time an [`Event`](crate::net::simulation::event::Event) is appended to a party's trace, each
//! hook whose [`trigger`](crate::net::simulation::hook::TriggeredHook::trigger) matches that
//! event's [`EventType`](crate::net::simulation::event::EventType) runs. This is the extension
//! point for measuring a run (how many bytes each party put on the wire) or for steering it
//! (injecting a reply when a party receives a particular message).
//!
//! [`MetricHook`](crate::net::simulation::hook::MetricHook) is the built-in example: it accumulates
//! the bytes sent, in total and per sending party.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::net::{
    simulation::{
        event::{Event, EventType},
        switchboard::Switchboard,
    },
    PartyId,
};

/// A hook that runs in reaction to events recorded during a simulation.
///
/// Hooks are registered through [`simulate`](crate::net::simulation::simulator::simulate) and fire
/// as each event is appended to a party's trace. They are the extension point for observing or
/// steering a run (for example, injecting a reply when a party receives a particular message).
///
/// `run` is handed `&mut Switchboard`, but only the switchboard's public API is reachable, so a hook cannot corrupt
/// the event queue or recurse back into the recording path.
pub trait TriggeredHook: Send + Sync {
    /// The event type this hook reacts to, or `None` to react to *every* event.
    fn trigger(&self) -> Option<EventType>;
    /// Runs the hook for `party_id` against the just-recorded `event`, with access to the
    /// `switchboard`'s public API.
    fn run(&self, party_id: PartyId, event: &Event, switchboard: &mut Switchboard);
}

/// A [`TriggeredHook`] that totals the bytes each party sends during a simulation.
///
/// It reacts to [`EventType::SendData`] only, so it measures what each party *put on the wire*,
/// counting the payload size of every packet at the moment it is handed to the network — a packet
/// in flight is already counted, and a packet is never counted twice on delivery. Sends from a
/// party to itself are excluded: they never touch the wire, because
/// [`TcpNetwork`](crate::net::tcp::TcpNetwork) delivers them over an in-process loop-back channel
/// rather than a socket. The counters are **byte counts, not message counts**.
///
/// The counters live behind `Arc<Mutex<_>>` because a hook is registered as an
/// `Arc<dyn TriggeredHook>` and only ever sees `&self`. Register a clone of the `Arc` with
/// [`simulate`](crate::net::simulation::simulator::simulate) and read the totals back through
/// [`total_data`](MetricHook::total_data) and [`total_data_by`](MetricHook::total_data_by) once the
/// run has finished:
///
/// ```rust
/// use std::collections::HashMap;
/// use std::sync::{Arc, Mutex};
///
/// use scl_rs::net::simulation::hook::MetricHook;
///
/// let metrics = Arc::new(MetricHook::new(
///     Arc::new(Mutex::new(0_usize)),
///     Arc::new(Mutex::new(HashMap::new())),
/// ));
///
/// // Hand `metrics.clone()` to `simulate` as one of its hooks, then read the counters back here.
/// assert_eq!(metrics.total_data(), 0);
/// ```
pub struct MetricHook {
    total_data: Arc<Mutex<usize>>,
    total_data_per_party: Arc<Mutex<HashMap<PartyId, usize>>>,
}

impl MetricHook {
    /// Creates a hook that accumulates into the given counters.
    ///
    /// Both are shared handles so that the caller keeps a view of the totals after the hook has
    /// been moved into [`simulate`](crate::net::simulation::simulator::simulate). They should start
    /// at zero and empty, respectively; a non-zero start is simply added to.
    pub fn new(
        total_data: Arc<Mutex<usize>>,
        total_data_per_party: Arc<Mutex<HashMap<PartyId, usize>>>,
    ) -> Self {
        Self {
            total_data,
            total_data_per_party,
        }
    }

    /// Returns the total number of bytes sent by all parties.
    pub fn total_data(&self) -> usize {
        *self.total_data.lock().expect("lock free")
    }

    /// Returns the number of bytes sent by `party_id`, or `None` if that party never sent anything.
    pub fn total_data_by(&self, party_id: &PartyId) -> Option<usize> {
        self.total_data_per_party
            .lock()
            .expect("lock free")
            .get(party_id)
            .copied()
    }
}

impl TriggeredHook for MetricHook {
    fn trigger(&self) -> Option<EventType> {
        Some(EventType::SendData)
    }

    fn run(&self, _party: PartyId, event: &Event, _switchboard: &mut Switchboard) {
        if let Event::SendData { link, size, .. } = event {
            // Self-sends never touch the wire (TcpNetwork loops them back in-process), so they
            // don't count as bandwidth.
            if link.sender() == link.recipient() {
                return;
            }
            *self.total_data.lock().expect("lock free") += *size;
            *self
                .total_data_per_party
                .lock()
                .expect("lock free")
                .entry(link.sender())
                .or_default() += *size;
        }
    }
}
