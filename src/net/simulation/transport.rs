use crate::net;
use crate::net::channel::ChannelError::{ChannelNotFound, EmptyBuffer};
use crate::net::simulation::channel::ChannelId;
use crate::net::{Network, Packet, PartyId};
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

#[derive(Default)]
pub struct SimulatedNetwork {
    /// List of channel queues.
    ///
    /// In this method, [`Packet`] is behind an [`Arc`] given that we don't copy packets at
    /// broadcasting. Instead, we have counted references to that packet so that the reality is that
    /// there is only one packet and the other parties hold references.
    channels: HashMap<ChannelId, VecDeque<Arc<Packet>>>,
    /// The party ID on this network.
    party_id: PartyId,
}

#[async_trait]
impl Network for SimulatedNetwork {
    async fn send_to(&mut self, party_id: PartyId, packet: &Packet) -> net::Result<usize> {
        let channel = ChannelId::new(self.party_id, party_id);
        let channel_buffer = self
            .channels
            .get_mut(&channel)
            .ok_or(ChannelNotFound(channel))?;
        for stored_packet in channel_buffer.iter() {
            if stored_packet == packet {
                channel_buffer.push_back(stored_packet.clone());
                return Ok(packet.size());
            }
        }
        channel_buffer.push_back(Arc::new(packet.clone()));
        Ok(packet.size())
    }

    async fn recv_from(&mut self, party_id: PartyId) -> net::Result<Packet> {
        let channel_id = ChannelId::new(self.party_id, party_id);
        let packet = self
            .channels
            .get_mut(&channel_id)
            .ok_or(ChannelNotFound(channel_id))?
            .pop_front()
            .ok_or(EmptyBuffer)?;
        Ok(packet.as_ref().clone())
    }

    async fn close(&mut self) -> net::Result<()> {
        self.channels.drain();
        Ok(())
    }
}

impl SimulatedNetwork {
    pub fn has_data(&self, channel_id: ChannelId) -> bool {
        if self.channels.contains_key(&channel_id) {
            !self.channels[&channel_id].is_empty()
        } else {
            false
        }
    }
}
