use crate::net::simulation::channel::{ChannelId, NetworkConfig, SimpleNetworkConfig};
use crate::net::simulation::context::Error::PartyNotFound;
use crate::net::simulation::event::Event;
use crate::net::simulation::hook::TriggeredHook;
use crate::net::simulation::SimulationTrace;
use crate::net::{Network, PartyId};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

const TCP_IP_HEADER_SIZE: usize = 40;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Party {0:?} not found")]
    PartyNotFound(PartyId),
}

pub struct SimulationContext<N: NetworkConfig> {
    /// IDs for parties in the simulation.
    party_ids: Vec<PartyId>,
    /// Configuration of the network.
    network_config: N,
    /// Simulation trace for each party during protocol execution.
    traces: HashMap<PartyId, SimulationTrace>,
    /// Individual clock for each party.
    party_clocks: HashMap<PartyId, Instant>,

    sends: HashMap<ChannelId, VecDeque<Duration>>,
    /// Tells whether the some party is receiving data from the other party.
    ///
    /// For example, if PartyId A is receiving information from PartyId B, then we insert
    /// `ChannelId { local: A, remote: B }` into the [`HashSet`]. If the channel is not in the
    /// [`HashSet`], it is because it is not receiving.
    recv_data_tracker: HashSet<ChannelId>,
    /// Tracker for cancellation events.
    ///
    /// If the party has called a cancelation, then the party will appear in the [`HashSet`].
    cancellation_tracker: HashSet<PartyId>,
    hooks: Vec<Arc<TriggeredHook<N>>>,
}

impl<N: NetworkConfig> SimulationContext<N> {
    pub fn n_parties(&self) -> usize {
        self.party_ids.len()
    }

    pub fn network_config(&self) -> &N {
        &self.network_config
    }

    pub fn start_clock(&mut self, party_id: PartyId) {
        self.party_clocks.insert(party_id, Instant::now());
    }

    pub fn record_event(&mut self, party_id: PartyId, event: Event) {
        self.traces
            .entry(party_id)
            .or_insert(SimulationTrace::empty())
            .0
            .push(event);
    }

    pub fn send(&mut self, sender: PartyId, receiver: PartyId, timestamp: Duration) {
        let channel_id = ChannelId::new(sender, receiver);
        self.sends
            .entry(channel_id)
            .or_default()
            .push_back(timestamp);
    }

    pub fn is_receiving(&self, receiver: PartyId, sender: PartyId) -> bool {
        let channel_id = ChannelId::new(receiver, sender);
        self.recv_data_tracker.contains(&channel_id)
    }

    pub fn recv_start(&mut self, receiver: PartyId, sender: PartyId) {
        let channel_id = ChannelId::new(receiver, sender);
        self.recv_data_tracker.insert(channel_id);
    }

    pub fn recv_done(&mut self, receiver: PartyId, sender: PartyId) {
        let channel_id = ChannelId::new(receiver, sender);
        self.recv_data_tracker.remove(&channel_id);
    }

    pub fn is_dead(&self, party_id: PartyId) -> Result<bool, Error> {
        match self.traces.get(&party_id) {
            Some(trace) => {
                if trace.0.is_empty() {
                    Ok(true)
                } else {
                    // SAFETY: We already know that traces.0 is not empty.
                    let last_event_type = trace.0.last().unwrap();
                    match last_event_type {
                        Event::Stop { .. } | Event::Killed { .. } | Event::Cancelled { .. } => {
                            Ok(true)
                        }
                        _ => Ok(false),
                    }
                }
            }
            None => Err(PartyNotFound(party_id)),
        }
    }

    pub fn elapsed_time_for_party(&self, party_id: PartyId) -> Result<Duration, Error> {
        let most_recent = self.last_event_timestamp(party_id)?;
        let current_instant_of_party = match self.party_clocks.get(&party_id) {
            Some(instant) => *instant,
            None => return Err(PartyNotFound(party_id)),
        };

        Ok(most_recent + (Instant::now() - current_instant_of_party))
    }

    fn last_event_timestamp(&self, party_id: PartyId) -> Result<Duration, Error> {
        match self.traces.get(&party_id) {
            Some(trace) => {
                if trace.0.is_empty() {
                    Ok(Duration::ZERO)
                } else {
                    // SAFETY: We already know that traces.0 is not empty.
                    let last_event = trace.0.last().unwrap();
                    Ok(last_event.timestamp())
                }
            }
            None => Err(PartyNotFound(party_id)),
        }
    }

    pub fn current_time_of_party(&self, party_id: PartyId) -> Result<Duration, Error> {
        self.last_event_timestamp(party_id)
    }

    pub fn trace(&self, party_id: PartyId) -> &SimulationTrace {
        &self.traces[&party_id]
    }

    pub fn cancel_party(&mut self, party_id: PartyId) {
        self.cancellation_tracker.insert(party_id);
    }

    pub fn cancel_simulation(&mut self) {
        for party_id in self.party_ids.iter() {
            self.cancellation_tracker.insert(*party_id);
        }
    }

    pub fn new(
        party_ids: Vec<PartyId>,
        network_config: N,
        hooks: Vec<Arc<TriggeredHook<N>>>,
    ) -> Self {
        Self {
            party_ids,
            network_config,
            traces: HashMap::new(),
            party_clocks: HashMap::new(),
            sends: HashMap::new(),
            recv_data_tracker: HashSet::new(),
            cancellation_tracker: HashSet::new(),
            hooks,
        }
    }
}

fn size_with_headers_in_bits(bytes: usize, mss: usize) -> usize {
    let num_packets = bytes.div_ceil(mss);
    8 * (bytes + num_packets * TCP_IP_HEADER_SIZE)
}
