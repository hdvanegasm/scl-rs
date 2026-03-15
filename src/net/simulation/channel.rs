use crate::net;
use crate::net::channel::{Channel, ChannelError};
use crate::net::simulation::context;
use crate::net::simulation::context::SimulationContext;
use crate::net::simulation::event::Event;
use crate::net::simulation::transport::SimulatedNetwork;
use crate::net::{Network, NetworkError, Packet, PartyId};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid configuration parameters for the channel: {0:?}")]
    InvalidConfig(ChannelConfigBuilder),

    #[error("error in the context: {0:?}")]
    ContextError(#[from] context::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Hash, PartialEq, PartialOrd, Debug, Eq, Copy, Clone)]
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

pub trait NetworkConfig: Clone + Send + Sync {
    fn channel_config(&self, channel_id: ChannelId) -> Result<ChannelConfig>;
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Bandwidth(usize);

/// RTT of the network in milliseconds.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Rtt(usize);

impl Rtt {
    pub fn to_secs(&self) -> f64 {
        self.0 as f64 / 1000.0
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Mss(usize);

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct PackageLoss(f64);

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct WindowSize(usize);

/// Helper macro to implement inner value manipulation methods for the metrics.
macro_rules! inner_value_manipulations {
    ($struct_name:ident, $inner_type:ident) => {
        impl $struct_name {
            /// Creates a new instance with the corresponding value.
            pub fn new(value: $inner_type) -> Self {
                Self(value)
            }

            /// Gets the inner value in the type.
            pub fn value(&self) -> $inner_type {
                self.0
            }

            /// Modifies the inner value.
            pub fn set_value(&mut self, value: $inner_type) {
                self.0 = value;
            }
        }
    };
}

inner_value_manipulations!(Bandwidth, usize);
inner_value_manipulations!(Rtt, usize);
inner_value_manipulations!(Mss, usize);
inner_value_manipulations!(PackageLoss, f64);
inner_value_manipulations!(WindowSize, usize);

#[derive(Debug, PartialEq)]
pub enum NetworkType {
    /// A network where the channels are TCP channels.
    Tcp,
    /// A network where the communication is done instantly.
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
    const TCP_IP_HEADER_SIZE: usize = 40;

    pub fn new(
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

    pub fn lossless_throughput(&self) -> f64 {
        let rtt = self.rtt.to_secs();
        let wind_size = 8.0 * self.window_size.value() as f64;
        let max_throughput = wind_size / rtt;

        let bandwidth = self.bandwidth.value() as f64;
        let actual_throughput = f64::min(bandwidth, max_throughput);
        actual_throughput
    }

    pub fn lossy_throughput(&self) -> f64 {
        let mss = self.mss.value() as f64;
        let loss_term = f64::sqrt(3.0 / (2.0 * self.package_loss.value()));
        let rtt = self.rtt.to_secs();
        loss_term * (8.0 * mss / rtt)
    }

    pub fn recv_time_tcp(&self, n: usize) -> Duration {
        let total_size_bits = self.size_with_header_bits(n);
        let mut actual_throughput = self.lossless_throughput();
        if self.package_loss.value() > 0.0 {
            let throughput_loss = self.lossy_throughput();
            actual_throughput = f64::min(throughput_loss, actual_throughput);
        }
        let t = total_size_bits / actual_throughput + self.rtt.to_secs();
        Duration::from_secs_f64(t)
    }

    pub fn size_with_header_bits(&self, n_bytes: usize) -> f64 {
        let num_packets = f64::ceil(n_bytes as f64 / self.mss.value() as f64);
        8.0 * (n_bytes as f64 + num_packets * Self::TCP_IP_HEADER_SIZE as f64)
    }

    pub fn adjust_send_time(&self, send_time: Duration, n: usize) -> Duration {
        match self.net_type {
            NetworkType::Tcp => send_time + self.recv_time_tcp(n),
            NetworkType::Instant => send_time,
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

    pub fn build(self) -> Result<ChannelConfig> {
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

impl Default for ChannelConfigBuilder {
    fn default() -> Self {
        Self {
            net_type: Self::DEFAULT_NET_TYPE,
            bandwidth: Self::DEFAULT_BANDWIDTH,
            rtt: Self::DEFAULT_RTT,
            mss: Self::DEFAULT_MSS,
            package_loss: Self::DEFAULT_PACKAGE_LOSS,
            window_size: Self::DEFAULT_WINDOW_SIZE,
        }
    }
}

pub struct SimulatedChannel<N: NetworkConfig> {
    id: ChannelId,
    context: Arc<Mutex<SimulationContext<N>>>,
    transport: Arc<Mutex<SimulatedNetwork>>,
}

impl<N> SimulatedChannel<N>
where
    N: NetworkConfig,
{
    pub fn new(
        end_point_a: PartyId,
        end_point_b: PartyId,
        transport: Arc<Mutex<SimulatedNetwork>>,
        context: Arc<Mutex<SimulationContext<N>>>,
    ) -> Self {
        let channel_id = ChannelId::new(end_point_a, end_point_b);
        Self {
            id: channel_id,
            transport,
            context,
        }
    }

    pub async fn has_data(&self) -> Result<bool> {
        // Save the event and update execute the command in the transport.
        let now = {
            let mut ctxt_guard = self.context.lock().await;
            let elapsed_time = ctxt_guard.elapsed_time_for_party(self.id.local)?;
            ctxt_guard.record_event(
                self.id.local,
                Event::HasData {
                    timestamp: elapsed_time,
                    channel_id: self.id,
                },
            );
            elapsed_time
        };
        let has_data = {
            let transport_guard = self.transport.lock().await;
            transport_guard.has_data(self.id)
        };

        if !has_data {
            // Text taken from secure-computation-library.
            //
            // Here we have multiple cases:
            // 1. If the remote party is ahead of us in the execution, then the data it sends will
            //    first arrive at some point in the future.
            // 2. If the remote party is dead, we will not receive any data anymore.
            // 3. If the remote party is trying to receive data from us, we will not receive data
            //    until we send data to it first. Sending data to the remote party is not possible
            //    earlier than "now". Hence, we will not receive data from the remote party until
            //    some point after from "now".
            loop {
                // We encapsulate the context mutex in a scope to avoid deadlocks. If we don't do
                // this, the mutex will be locked when yield_now() is called. So other tasks will
                // not be able to acquire the lock.
                let ready = {
                    let ctxt_guard = self.context.lock().await;
                    let remote_ahead = now < ctxt_guard.current_time_of_party(self.id.remote)?;
                    let remote_dead = ctxt_guard.is_dead(self.id.remote)?;
                    let remote_receiving = ctxt_guard.is_receiving(self.id.remote, self.id.local);
                    remote_dead || remote_receiving || remote_ahead
                };
                if ready {
                    break;
                }
                tokio::task::yield_now().await;
            }
        }
        let mut ctxt_guard = self.context.lock().await;
        ctxt_guard.start_clock(self.id.local);
        Ok(true)
    }
}

#[async_trait]
impl<N> Channel for SimulatedChannel<N>
where
    N: NetworkConfig,
{
    async fn close(&mut self) -> crate::net::channel::Result<()> {
        let mut context_guard = self.context.lock().await;
        let elapsed_time = context_guard.elapsed_time_for_party(self.id.local)?;
        context_guard.record_event(
            self.id.local,
            Event::CloseChannel {
                timestamp: elapsed_time,
                channel_id: self.id,
            },
        );
        Ok(())
    }

    async fn send(&mut self, packet: &Packet) -> crate::net::channel::Result<usize> {
        {
            let mut context_guard = self.context.lock().await;
            let elapsed_time = context_guard.elapsed_time_for_party(self.id.local)?;

            context_guard.send(self.id.local, self.id.remote, elapsed_time);
            context_guard.record_event(
                self.id.local,
                Event::SendData {
                    timestamp: elapsed_time,
                    channel_id: self.id,
                    size: packet.size(),
                },
            )
        };

        {
            let mut transport_guard = self.transport.lock().await;
            transport_guard
                .send_to(self.id.remote, packet)
                .await
                .map_err(|error| ChannelError::NetworkError(Box::new(error)))?;
        };
        Ok(packet.size())
    }

    // TODO: Finish this.
    async fn recv(&mut self) -> crate::net::channel::Result<Packet> {
        let elapsed = {
            let mut ctxt_guard = self.context.lock().await;
            ctxt_guard.recv_start(self.id.local, self.id.remote);
            ctxt_guard.elapsed_time_for_party(self.id.local)?
        };

        // Wait until there is a packet on the transport.
        loop {
            let transport_guard = self.transport.lock().await;
            if transport_guard.has_data(self.id) {
                break;
            }
            tokio::task::yield_now().await;
        }

        let packet = {
            let mut transport_guard = self.transport.lock().await;
            transport_guard
                .recv_from(self.id.remote)
                .await
                .map_err(|error| ChannelError::NetworkError(Box::new(error)))?
        };

        let mut ctxt_guard = self.context.lock().await;
        ctxt_guard.recv_done(self.id.local, self.id.remote);
        Ok(packet)
    }
}

#[derive(Debug, Clone)]
pub struct SimpleNetworkConfig;

impl NetworkConfig for SimpleNetworkConfig {
    fn channel_config(&self, channel_id: ChannelId) -> Result<ChannelConfig> {
        let mut default_config = ChannelConfigBuilder::default();
        if channel_id.local == channel_id.remote {
            default_config = default_config.net_type(NetworkType::Instant);
        }
        default_config.build()
    }
}
