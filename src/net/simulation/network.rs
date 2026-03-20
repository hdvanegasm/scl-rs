use crate::net;
use crate::net::channel::ChannelError::EmptyBuffer;
use crate::net::channel::{Channel, ChannelError};
use crate::net::simulation::channel::{ChannelId, NetworkConfig, SimulatedChannel};
use crate::net::simulation::context::SimulationContext;
use crate::net::{Network, NetworkError, Packet, PartyId};
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default)]
pub struct Transport {
    /// List of channel queues.
    ///
    /// In this method, [`Packet`] is behind an [`Arc`] given that we don't copy packets at
    /// broadcasting. Instead, we have counted references to that packet so that the reality is that
    /// there is only one packet and the other parties hold references.
    channel_queues: HashMap<ChannelId, VecDeque<Arc<Packet>>>,
}

impl Transport {
    pub fn new(n_parties: usize) -> Self {
        let mut channels = HashMap::new();
        for i in 0..n_parties {
            for j in 0..n_parties {
                let channel_id = ChannelId::new(PartyId(i), PartyId(j));
                channels.insert(channel_id, VecDeque::new());
            }
        }
        Self {
            channel_queues: channels,
        }
    }

    pub fn has_data(&self, channel_id: ChannelId) -> bool {
        if self.channel_queues.contains_key(&channel_id) {
            !self.channel_queues[&channel_id].is_empty()
        } else {
            false
        }
    }

    pub fn send_to(
        &mut self,
        channel_id: ChannelId,
        packet: &Packet,
    ) -> Result<usize, ChannelError> {
        let channel_buffer = self
            .channel_queues
            .get_mut(&channel_id)
            .ok_or(ChannelError::ChannelNotFound(channel_id))?;
        for stored_packet in channel_buffer.iter() {
            if stored_packet == packet {
                channel_buffer.push_back(stored_packet.clone());
                return Ok(packet.size());
            }
        }
        channel_buffer.push_back(Arc::new(packet.clone()));
        Ok(packet.size())
    }

    pub fn recv(&mut self, channel_id: ChannelId) -> Result<Packet, ChannelError> {
        let packet = self
            .channel_queues
            .get_mut(&channel_id)
            .ok_or(ChannelError::ChannelNotFound(channel_id))?
            .pop_front()
            .ok_or(EmptyBuffer)?;
        Ok(packet.as_ref().clone())
    }
}

pub struct SimulatedNetwork<N: NetworkConfig> {
    local_party_id: PartyId,
    channels: HashMap<PartyId, SimulatedChannel<N>>,
}

impl<N: NetworkConfig> SimulatedNetwork<N> {
    pub fn new(
        party_id: PartyId,
        other_parties: Vec<PartyId>,
        transport: Arc<Mutex<Transport>>,
        context: Arc<Mutex<SimulationContext<N>>>,
    ) -> Self {
        let mut channels = HashMap::new();
        for other_id in other_parties {
            let channel =
                SimulatedChannel::new(party_id, other_id, transport.clone(), context.clone());
            channels.insert(other_id, channel);
        }
        Self {
            local_party_id: party_id,
            channels,
        }
    }
}

#[async_trait]
impl<N: NetworkConfig> Network for SimulatedNetwork<N> {
    async fn send_to(&mut self, party_id: PartyId, packet: &Packet) -> net::Result<usize> {
        let channel = self
            .channels
            .get_mut(&party_id)
            .ok_or(NetworkError::PartyNotFound(party_id))?;
        channel.send(packet).await?;
        Ok(packet.size())
    }

    async fn recv_from(&mut self, party_id: PartyId) -> net::Result<Packet> {
        let channel = self
            .channels
            .get_mut(&party_id)
            .ok_or(NetworkError::PartyNotFound(party_id))?;
        let packet = channel.recv().await?;
        Ok(packet)
    }

    async fn close(&mut self) -> net::Result<()> {
        for channel in self.channels.values_mut() {
            channel.close().await?;
        }
        Ok(())
    }

    fn local_party(&self) -> PartyId {
        self.local_party_id
    }
}
