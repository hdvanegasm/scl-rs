use crate::net::channel::Channel;
use crate::net::simulation::PartyId;
use crate::net::Packet;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid configuration parameters for the channel: {0:?}")]
    InvalidConfig(ChannelConfigBuilder),
}

#[derive(Hash, PartialEq, PartialOrd, Debug)]
pub struct ChannelId {
    pub local: PartyId,
    pub remote: PartyId,
}

impl ChannelId {
    pub fn new(local: PartyId, remote: PartyId) -> Self {
        ChannelId { local, remote }
    }

    pub fn flip_end_points(&self) -> Self {
        Self::new(self.remote.clone(), self.local.clone())
    }
}

pub trait NetworkConfig {
    fn get_channel_config(&self, channel_id: ChannelId) -> &ChannelConfig;
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Bandwidth(usize);

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Rtt(usize);

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Mss(usize);

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct PackageLoss(f64);

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct WindowSize(usize);

#[derive(Debug, PartialEq)]
pub enum NetworkType {
    Tcp,
    Instant,
}

#[derive(Debug)]
pub struct ChannelConfig {
    pub net_type: NetworkType,
    pub bandwidth: Bandwidth,
    pub rtt: Rtt,
    pub mss: Mss,
    pub package_loss: PackageLoss,
    pub window_size: WindowSize,
}

impl ChannelConfig {
    pub(crate) fn new(
        net_type: NetworkType,
        bandwidth: Bandwidth,
        rtt: Rtt,
        mss: Mss,
        package_loss: PackageLoss,
        window_size: WindowSize,
    ) -> Self {
        Self {
            net_type,
            bandwidth,
            mss,
            rtt,
            package_loss,
            window_size,
        }
    }
}

#[derive(Debug)]
pub struct ChannelConfigBuilder {
    pub net_type: NetworkType,
    pub bandwidth: Bandwidth,
    pub rtt: Rtt,
    pub mss: Mss,
    pub package_loss: PackageLoss,
    pub window_size: WindowSize,
}

impl ChannelConfigBuilder {
    const DEFAULT_NET_TYPE: NetworkType = NetworkType::Tcp;
    const DEFAULT_BANDWIDTH: Bandwidth = Bandwidth(1000000);
    const DEFAULT_RTT: Rtt = Rtt(100);
    const DEFAULT_MSS: Mss = Mss(1460);
    const DEFAULT_PACKAGE_LOSS: PackageLoss = PackageLoss(0.0);
    const DEFAULT_WINDOW_SIZE: WindowSize = WindowSize(65536);

    pub fn new() -> Self {
        Self {
            net_type: Self::DEFAULT_NET_TYPE,
            bandwidth: Self::DEFAULT_BANDWIDTH,
            rtt: Self::DEFAULT_RTT,
            mss: Self::DEFAULT_MSS,
            package_loss: Self::DEFAULT_PACKAGE_LOSS,
            window_size: Self::DEFAULT_WINDOW_SIZE,
        }
    }

    pub fn net_type(self, net_type: NetworkType) -> Self {
        Self { net_type, ..self }
    }

    pub fn bandwidth(self, bandwidth: Bandwidth) -> Self {
        Self { bandwidth, ..self }
    }

    pub fn rtt(self, rtt: Rtt) -> Self {
        Self { rtt, ..self }
    }

    pub fn mss(self, mss: Mss) -> Self {
        Self { mss, ..self }
    }

    pub fn package_loss(self, package_loss: PackageLoss) -> Self {
        Self {
            package_loss,
            ..self
        }
    }

    pub fn window_size(self, window_size: WindowSize) -> Self {
        Self {
            window_size,
            ..self
        }
    }

    pub fn build(self) -> Result<ChannelConfig, Error> {
        if self.is_valid() {
            Ok(ChannelConfig::new(
                self.net_type,
                self.bandwidth,
                self.rtt,
                self.mss,
                self.package_loss,
                self.window_size,
            ))
        } else {
            Err(Error::InvalidConfig(self))
        }
    }

    pub fn is_valid(&self) -> bool {
        todo!()
    }
}

pub struct SimulatedChannel;

impl SimulatedChannel {
    pub fn has_data(&self) -> bool {
        todo!()
    }
}

impl Channel for SimulatedChannel {
    fn shutdown(&mut self) -> crate::net::channel::Result<()> {
        todo!()
    }

    fn send(&mut self, packet: &Packet) -> crate::net::channel::Result<usize> {
        todo!()
    }

    fn recv(&mut self) -> crate::net::channel::Result<Packet> {
        todo!()
    }
}
