use crate::net::simulation::{Result, SimulationError};
use crate::net::PartyId;
use std::time::Duration;

/// An ID for a channel connecting two parties.
///
/// The ID of the channel consists of the pair of the two parties connected through the channel: the
/// local party, and the remote party.
#[derive(Hash, PartialEq, PartialOrd, Debug, Eq, Copy, Clone)]
pub struct ChannelId {
    local: PartyId,
    remote: PartyId,
}

impl ChannelId {
    /// Creates a new channel.
    pub fn new(local: PartyId, remote: PartyId) -> Self {
        ChannelId { local, remote }
    }

    /// Returns the channel ID consisting of the end-points of the channel flipped.
    pub fn flip_end_points(&self) -> Self {
        Self::new(self.remote.clone(), self.local.clone())
    }

    /// Returns the ID of the local node in the channel ID.
    pub fn local(&self) -> PartyId {
        self.local
    }

    /// Returns the ID of the remote node in the channel ID.
    pub fn remote(&self) -> PartyId {
        self.remote
    }
}

/// Configuration of a network.
pub trait NetworkConfig: Clone + Send + Sync {
    /// Returns the configuration for a specific channel.
    fn channel_config(&self, channel_id: ChannelId) -> ChannelConfig;
}

/// Bandwidth used in this channel.
// TODO: complete the doc with the measurement unit of the bandwidth (e.g. bytes/s or bits/s).
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Bandwidth(usize);

/// RTT of the network in milliseconds.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Rtt(usize);

impl Rtt {
    /// Transform the RTT into seconds.
    pub fn to_secs(&self) -> f64 {
        self.0 as f64 / 1000.0
    }
}

/// Maximum segment size (MSS) of the channel, in bytes.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Mss(usize);

/// Fraction of packages loss.
///
/// This is a number betewen 0 and 1.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct PackageLoss(f64);

/// TCP window size of the channel, in bytes.
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

/// Type of the network that will be used in the execution.
#[derive(Debug, PartialEq, Clone)]
pub enum NetworkType {
    /// A network where the channels are TCP channels.
    Tcp,
    /// A network where the communication is done instantly.
    Instant,
}

/// Configuration for a channel.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// The network type for this channel.
    pub net_type: NetworkType,
    /// The bandwidth of the channel.
    pub bandwidth: Bandwidth,
    /// The RTT of the channel.
    pub rtt: Rtt,
    /// The MSS of the channel.
    pub mss: Mss,
    /// The fraction of package loss for this channel.
    pub package_loss: PackageLoss,
    /// The window size of the channel.
    pub window_size: WindowSize,
}

impl ChannelConfig {
    /// Size of the TCP header in bytes.
    const TCP_IP_HEADER_SIZE: usize = 40;

    /// Creates a new configuration for a channel with the provided configuration.
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

    fn lossless_throughput(&self) -> f64 {
        let rtt = self.rtt.to_secs();
        let wind_size = 8.0 * self.window_size.value() as f64;
        let max_throughput = wind_size / rtt;

        let bandwidth = self.bandwidth.value() as f64;
        let actual_throughput = f64::min(bandwidth, max_throughput);
        actual_throughput
    }

    fn lossy_throughput(&self) -> f64 {
        let mss = self.mss.value() as f64;
        let loss_term = f64::sqrt(3.0 / (2.0 * self.package_loss.value()));
        let rtt = self.rtt.to_secs();
        loss_term * (8.0 * mss / rtt)
    }

    fn recv_time_tcp(&self, n: usize) -> Duration {
        let total_size_bits = self.size_with_header_bits(n);
        let mut actual_throughput = self.lossless_throughput();
        if self.package_loss.value() > 0.0 {
            let throughput_loss = self.lossy_throughput();
            actual_throughput = f64::min(throughput_loss, actual_throughput);
        }
        let t = total_size_bits / actual_throughput + self.rtt.to_secs();
        Duration::from_secs_f64(t)
    }

    fn size_with_header_bits(&self, n_bytes: usize) -> f64 {
        let num_packets = f64::ceil(n_bytes as f64 / self.mss.value() as f64);
        8.0 * (n_bytes as f64 + num_packets * Self::TCP_IP_HEADER_SIZE as f64)
    }

    /// Network delay to send a message of `n_bytes` bytes.
    pub fn message_delay(&self, n_bytes: usize) -> Duration {
        match self.net_type {
            NetworkType::Tcp => self.recv_time_tcp(n_bytes),
            NetworkType::Instant => Duration::ZERO,
        }
    }
}

/// A builder for a channel configuration.
#[derive(Debug)]
pub struct ChannelConfigBuilder {
    /// The network type of this channel.
    pub net_type: NetworkType,
    /// The bandwidth for this channel.
    pub bandwidth: Bandwidth,
    /// The RTT for this channel.
    pub rtt: Rtt,
    /// The maximum segment size between two channels.
    pub mss: Mss,
    /// The proportion of lost packages during a connection.
    pub package_loss: PackageLoss,
    /// The window size.
    pub window_size: WindowSize,
}

impl ChannelConfigBuilder {
    const DEFAULT_NET_TYPE: NetworkType = NetworkType::Tcp;
    const DEFAULT_BANDWIDTH: Bandwidth = Bandwidth(1000000);
    const DEFAULT_RTT: Rtt = Rtt(100);
    const DEFAULT_MSS: Mss = Mss(1460);
    const DEFAULT_PACKAGE_LOSS: PackageLoss = PackageLoss(0.0);
    const DEFAULT_WINDOW_SIZE: WindowSize = WindowSize(65536);

    /// Sets the network type to the provided for the configuration.
    pub fn net_type(self, net_type: NetworkType) -> Self {
        Self { net_type, ..self }
    }

    /// Sets the bandwidth to the given value.
    pub fn bandwidth(self, bandwidth: Bandwidth) -> Self {
        Self { bandwidth, ..self }
    }

    /// Sets the RTT to the given value.
    pub fn rtt(self, rtt: Rtt) -> Self {
        Self { rtt, ..self }
    }

    /// Sets the maximum segment size (MSS) to the given value.
    pub fn mss(self, mss: Mss) -> Self {
        Self { mss, ..self }
    }

    /// Sets the fraction of lost packages to the given value.
    pub fn package_loss(self, package_loss: PackageLoss) -> Self {
        Self {
            package_loss,
            ..self
        }
    }

    /// Sets the TCP window size to the given value.
    pub fn window_size(self, window_size: WindowSize) -> Self {
        Self {
            window_size,
            ..self
        }
    }

    /// Builds the [`ChannelConfig`] from the configured values.
    ///
    /// # Errors
    ///
    /// Returns [`SimulationError::InvalidConfig`] if the configured values are not valid (see
    /// [`is_valid`](Self::is_valid)).
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
            Err(SimulationError::InvalidConfig(self))
        }
    }

    /// Returns whether the configured values form a valid channel configuration.
    ///
    /// A configuration is valid when the bandwidth, MSS, and window size are all non-zero and the
    /// package loss is a fraction in `[0, 1]`.
    pub fn is_valid(&self) -> bool {
        if self.bandwidth.value() == 0 {
            return false;
        } else if self.mss.value() == 0 {
            return false;
        } else if self.package_loss.value() < 0.0 {
            return false;
        } else if self.package_loss.value() > 1.0 {
            return false;
        } else if self.window_size.value() == 0 {
            return false;
        }
        true
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

/// A default [`NetworkConfig`] that gives every inter-party channel the
/// [`ChannelConfigBuilder`] defaults, and makes a party's channel to itself
/// [`Instant`](NetworkType::Instant) (zero delay).
#[derive(Debug, Clone)]
pub struct SimpleNetworkConfig;

impl NetworkConfig for SimpleNetworkConfig {
    fn channel_config(&self, channel_id: ChannelId) -> ChannelConfig {
        let mut default_config = ChannelConfigBuilder::default();
        if channel_id.local == channel_id.remote {
            default_config = default_config.net_type(NetworkType::Instant);
        }
        // SAFETY: The default values are correct. Hence, this will not panic.
        default_config.build().unwrap()
    }
}
