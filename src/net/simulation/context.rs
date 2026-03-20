use crate::net::simulation::channel::{ChannelId, NetworkConfig, SimpleNetworkConfig};
use crate::net::simulation::event::Event;
use crate::net::simulation::hook::TriggeredHook;
use crate::net::simulation::SimulationTrace;
use crate::net::simulation::{Result, SimulationError};
use crate::net::PartyId;
use std::cmp::max;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub struct SimulationContext<N: NetworkConfig> {
    /// IDs for parties in the simulation.
    party_ids: Vec<PartyId>,
    /// Configuration of the network.
    network_config: N,
    /// Simulation trace for each party during protocol execution.
    traces: HashMap<PartyId, SimulationTrace>,
    /// Individual clock for each party.
    party_clocks: HashMap<PartyId, Instant>,
    /// Tracker for sends.
    sends: HashMap<ChannelId, VecDeque<Duration>>,
    /// Tells whether some party is receiving data from the other party.
    ///
    /// For example, if PartyId A is receiving information from PartyId B, then we insert
    /// `ChannelId { local: A, remote: B }` into the [`HashSet`]. If the channel is not in the
    /// [`HashSet`], it is because it is not receiving.
    recv_data_tracker: HashSet<ChannelId>,
    /// Tracker for cancellation events.
    ///
    /// If the party has called a cancellation, then the party will appear in the [`HashSet`].
    cancellation_tracker: HashSet<PartyId>,
    /// Hooks that are triggered during the simulation.
    hooks: Vec<Arc<dyn TriggeredHook<N>>>,
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

    pub fn send(&mut self, sender: PartyId, receiver: PartyId, timestamp: Duration) -> Result<()> {
        let channel_id = ChannelId::new(sender, receiver);
        self.sends
            .get_mut(&channel_id)
            .ok_or(SimulationError::ChannelNotFound {
                id: channel_id,
                err_context: "while invoking send in the simulation context",
            })?
            .push_back(timestamp);
        Ok(())
    }

    pub fn recv(
        &mut self,
        receiver: PartyId,
        sender: PartyId,
        num_bytes: usize,
        timestamp: Duration,
    ) -> Result<Duration> {
        let channel_id = ChannelId::new(receiver, sender);
        let send_time = self
            .sends
            .get_mut(&channel_id)
            .ok_or(SimulationError::ChannelNotFound {
                id: channel_id,
                err_context: "while invoking recv in the simulation context",
            })?
            .pop_front()
            .ok_or(SimulationError::SendsEmpty)?;

        Ok(max(
            self.network_config
                .channel_config(channel_id)
                .adjust_send_time(send_time, num_bytes),
            timestamp,
        ))
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

    pub fn is_dead(&self, party_id: PartyId) -> Result<bool> {
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
            None => Err(SimulationError::PartyNotFound(party_id)),
        }
    }

    pub fn elapsed_time_for_party(&self, party_id: PartyId) -> Result<Duration> {
        let most_recent = self.last_event_timestamp(party_id)?;
        let current_instant_of_party = match self.party_clocks.get(&party_id) {
            Some(instant) => *instant,
            None => return Err(SimulationError::PartyNotFound(party_id)),
        };

        Ok(most_recent + (Instant::now() - current_instant_of_party))
    }

    pub fn last_event_timestamp(&self, party_id: PartyId) -> Result<Duration> {
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
            None => Err(SimulationError::PartyNotFound(party_id)),
        }
    }

    pub fn current_time_of_party(&self, party_id: PartyId) -> Result<Duration> {
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
        hooks: Vec<Arc<dyn TriggeredHook<N>>>,
    ) -> Self {
        // Create empty sends HashMap.
        let mut sends = HashMap::new();
        let mut traces = HashMap::new();
        let party_clocks = HashMap::new();

        for party_id in &party_ids {
            traces.insert(*party_id, SimulationTrace::empty());
            for other_id in &party_ids {
                let channel_id = ChannelId::new(*party_id, *other_id);
                sends.insert(channel_id, VecDeque::new());
            }
        }

        Self {
            party_ids,
            network_config,
            traces,
            party_clocks,
            sends,
            recv_data_tracker: HashSet::new(),
            cancellation_tracker: HashSet::new(),
            hooks,
        }
    }
}

pub async fn record_event<N: NetworkConfig>(
    party_id: PartyId,
    event: Event,
    context: Arc<Mutex<SimulationContext<N>>>,
) {
    let hooks_to_run = {
        let mut context_guard = context.lock().await;
        context_guard
            .traces
            .entry(party_id)
            .or_insert(SimulationTrace::empty())
            .add_event(event.clone());

        context_guard
            .hooks
            .iter()
            .filter(|hook| hook.trigger().is_none() && hook.trigger() == Some(event.event_type()))
            .cloned()
            .collect::<Vec<_>>()
    };

    for hook in hooks_to_run {
        hook.run(party_id, context.clone());
    }
}
