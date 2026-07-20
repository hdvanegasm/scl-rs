//! Party-to-party links and the network model that assigns each message a virtual-time delay.
//!
//! A [`Link`] is the directed `sender -> recipient` channel used as the routing key throughout the
//! simulator. A [`NetworkConfig`](crate::net::simulation::channel::NetworkConfig) maps each link to a
//! [`ChannelConfig`](crate::net::simulation::channel::ChannelConfig) describing its bandwidth,
//! RTT, MSS, packet loss and window size; [`ChannelConfig::message_delay`](crate::net::simulation::channel::ChannelConfig::message_delay)
//! turns those, together
//! with a message size, into the transit [`Duration`](std::time::Duration) the switchboard schedules
//! deliveries with. [`SimpleNetworkConfig`](crate::net::simulation::channel::SimpleNetworkConfig)
//! applies one channel configuration to every inter-party link (with zero-delay self-links), either
//! the builder defaults or its [`lan`](crate::net::simulation::channel::SimpleNetworkConfig::lan)
//! and [`wan`](crate::net::simulation::channel::SimpleNetworkConfig::wan) presets, and
//! [`ChannelConfigBuilder`] builds custom per-link configurations.

use crate::net::simulation::{Result, SimulationError};
use crate::net::PartyId;
use std::time::Duration;

/// A directed link between two parties: the channel that carries packets from `sender` to
/// `recipient`.
///
/// A `Link` is the single identifier for a party-to-party channel throughout the simulator: it is
/// the routing key for queued messages, the value recorded in the send/receive
/// [`Event`](crate::net::simulation::event::Event)s, and the key passed to
/// [`NetworkConfig::channel_config`] when timing a message. Because it is *directed* (`sender` →
/// `recipient`), a [`NetworkConfig`] may give the two orientations of a party pair different
/// characteristics (asymmetric up/down links); a symmetric configuration simply ignores the
/// orientation.
#[derive(Hash, PartialEq, Eq, Copy, Clone, Debug)]
pub struct Link {
    sender: PartyId,
    recipient: PartyId,
}

impl Link {
    /// Creates the directed link that carries packets from `sender` to `recipient`.
    pub fn new(sender: PartyId, recipient: PartyId) -> Self {
        Link { sender, recipient }
    }

    /// The party that sends on this link.
    pub fn sender(&self) -> PartyId {
        self.sender
    }

    /// The party that receives on this link.
    pub fn recipient(&self) -> PartyId {
        self.recipient
    }
}

/// Configuration of a network.
pub trait NetworkConfig: Clone + Send + Sync {
    /// Returns the configuration for the directed `link` (`sender` → `recipient`).
    ///
    /// The link is directed, so a configuration may return different characteristics for the two
    /// orientations of the same party pair; a symmetric network ignores the direction.
    fn channel_config(&self, link: Link) -> ChannelConfig;
}

/// Bandwidth used in this channel measured in bits per second.
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
/// This is a number between 0 and 1.
///
/// # Validity domain
///
/// A non-zero value switches the channel onto the square-root throughput formula,
/// `sqrt(3 / (2p))` packets per RTT. That formula is the standard, widely-used one and is
/// implemented here faithfully. The caveat is not the algebra but the question we ask of it: the
/// literature states it as an *asymptotic* result, valid as `p -> 0`, for the *almost-sure mean*
/// throughput of a *long-lived* TCP *Reno* flow. This crate instead reads a single number off it
/// to price one short message. Validation against `tc netem`-shaped runs measured what that costs:
///
/// - **Not long-lived.** The mean is a limit as the flow's length in RTTs grows. A 1 MB message
///   over a 100 ms RTT link lasts one to four AIMD sawtooth cycles and is dominated by slow start,
///   which the bare formula does not model. Real runs came in far faster than predicted — the
///   bandwidth-dominated transfer over-predicted by ~400 % against the median (~211 % against the
///   mean), while a round-dominated one on the same link stayed within 0.7 %.
/// - **Not Reno.** Linux defaults to CUBIC, which is more loss-tolerant and diverged from the
///   formula further than Reno did.
/// - **Not a mean.** 50 identical 1 % loss trials of the bandwidth-dominated transfer spanned
///   0.73 s to 7.11 s. The formula predicts an ensemble average; a deterministic simulator answers
///   with one number, and no single number is faithful to a spread that wide.
/// - **Not `p -> 0`.** Those runs used `p` of 1 % and 0.25 %.
///
/// So treat simulated timings on a lossy channel as order-of-magnitude only. Extensions covering
/// slow start, timeouts and non-Reno variants exist in the literature; none are implemented here.
///
/// Note also that the lossy path is *linearly* proportional to [`Mss`], where the loss-less path
/// uses the MSS only to price header overhead, so a mis-set MSS distorts a lossy channel far more
/// than a loss-less one.
///
/// See Loiseau et al., "Modeling TCP Throughput: an Elaborated Large-Deviations-Based Model and
/// its Empirical Validation", *Performance Evaluation*, 2010, eq. (1), which states the formula in
/// exactly this asymptotic, long-lived, Reno, mean-only form — and whose own subject is that the
/// mean alone does not characterize a single flow's deviations from it.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct PackageLoss(f64);

/// TCP window size of the channel, in bytes.
///
/// This is the *effective* end-to-end window — the unacknowledged data actually kept in flight —
/// and not a socket buffer size. The two are not interchangeable: a kernel derives its advertised
/// window from `SO_RCVBUF`/`tcp_rmem` through an overhead factor, and then only part of that window
/// is realized in steady state. Pinning `tcp_rmem` to 131,072 bytes on one Linux host and recovering
/// the window a real bulk transfer actually delivered gave ~70 KB — well under the 128 KiB
/// configured, and not a number any socket setting names directly.
///
/// It binds whenever the bandwidth-delay product exceeds it, and then it alone sets the rate:
/// throughput becomes `window * 8 / RTT` and [`Bandwidth`] is ignored entirely. On such a link,
/// leaving this at its 65,536-byte default misprices every message — on that host the default sat
/// ~7 % below the window the real loopback run delivered (see the crate-level "Benchmarks" section).
///
/// Calibrate it by measurement rather than by reading a buffer setting: time a bulk transfer of
/// known size over a link of known RTT, take its throughput `T`, and set this to `T * RTT / 8`.
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
        f64::min(bandwidth, max_throughput)
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
        // `message_delay` is the one-way time until the recipient receives the message, so the
        // propagation term is a single one-way hop (RTT/2), not a full round trip. The
        // serialization term uses the steady-state throughput formulas above.
        let t = total_size_bits / actual_throughput + self.rtt.to_secs() / 2.0;
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
        if self.bandwidth.value() == 0
            || self.mss.value() == 0
            || self.package_loss.value() < 0.0
            || self.package_loss.value() > 1.0
            || self.window_size.value() == 0
        {
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

/// A [`NetworkConfig`] that gives every inter-party channel the same [`ChannelConfig`], and makes
/// a party's channel to itself [`Instant`](NetworkType::Instant) (zero delay).
///
/// [`Default`] reproduces the [`ChannelConfigBuilder`] defaults; [`lan`](Self::lan) and
/// [`wan`](Self::wan) provide loss-less presets for the two deployment settings usually reported in
/// the MPC literature.
#[derive(Debug, Clone)]
pub struct SimpleNetworkConfig {
    channel_config: ChannelConfig,
}

impl SimpleNetworkConfig {
    /// Bandwidth of the [`lan`](Self::lan) preset: 1 Gbps.
    const LAN_BANDWIDTH: Bandwidth = Bandwidth(1_000_000_000);
    /// RTT of the [`lan`](Self::lan) preset: 1 ms.
    const LAN_RTT: Rtt = Rtt(1);
    /// Window size of the [`lan`](Self::lan) preset: 128 KiB, above the 125 kB BDP.
    const LAN_WINDOW_SIZE: WindowSize = WindowSize(131_072);

    /// Bandwidth of the [`wan`](Self::wan) preset: 100 Mbps.
    const WAN_BANDWIDTH: Bandwidth = Bandwidth(100_000_000);
    /// RTT of the [`wan`](Self::wan) preset: 100 ms.
    const WAN_RTT: Rtt = Rtt(100);
    /// Window size of the [`wan`](Self::wan) preset: the 1.25 MB BDP of the link above.
    const WAN_WINDOW_SIZE: WindowSize = WindowSize(1_250_000);

    /// Builds a configuration where every inter-party link uses `channel_config`.
    pub fn from_channel_config(channel_config: ChannelConfig) -> Self {
        Self { channel_config }
    }

    /// A loss-less LAN: 1 Gbps, 1 ms RTT, 1460-byte MSS, 128 KiB window.
    ///
    /// The window is chosen above the 125 kB bandwidth-delay product, so bandwidth is the binding
    /// constraint and the link delivers its nominal 1 Gbps.
    pub fn lan() -> Self {
        let channel_config = ChannelConfigBuilder::default()
            .bandwidth(Self::LAN_BANDWIDTH)
            .rtt(Self::LAN_RTT)
            .package_loss(PackageLoss::new(0.0))
            .window_size(Self::LAN_WINDOW_SIZE)
            .build()
            // SAFETY: the preset values are valid, so this cannot panic.
            .unwrap();
        Self { channel_config }
    }

    /// A loss-less WAN: 100 Mbps, 100 ms RTT, 1460-byte MSS, 1.25 MB window.
    ///
    /// The window is set to the bandwidth-delay product of the link, i.e. this models a *tuned*,
    /// window-scaled stack that actually realizes 100 Mbps over a long fat pipe. An untuned stack
    /// is window-bound instead: at the 64 KiB default the same link delivers ~5.2 Mbps. See
    /// [`WindowSize`] for how to calibrate it against a real deployment.
    pub fn wan() -> Self {
        let channel_config = ChannelConfigBuilder::default()
            .bandwidth(Self::WAN_BANDWIDTH)
            .rtt(Self::WAN_RTT)
            .package_loss(PackageLoss::new(0.0))
            .window_size(Self::WAN_WINDOW_SIZE)
            .build()
            // SAFETY: the preset values are valid, so this cannot panic.
            .unwrap();
        Self { channel_config }
    }
}

impl Default for SimpleNetworkConfig {
    fn default() -> Self {
        // SAFETY: the builder defaults are valid, so this cannot panic.
        Self::from_channel_config(ChannelConfigBuilder::default().build().unwrap())
    }
}

impl NetworkConfig for SimpleNetworkConfig {
    fn channel_config(&self, link: Link) -> ChannelConfig {
        let mut config = self.channel_config.clone();
        if link.sender == link.recipient {
            config.net_type = NetworkType::Instant;
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The directed link from party 0 to party 1.
    fn inter_party_link() -> Link {
        Link::new(PartyId::from(0_usize), PartyId::from(1_usize))
    }

    /// The link party 0 has to itself.
    fn self_link() -> Link {
        let p0 = PartyId::from(0_usize);
        Link::new(p0, p0)
    }

    /// Throughput a link is capped at by its window alone, in bits per second.
    fn window_bound(config: &ChannelConfig) -> f64 {
        8.0 * config.window_size.value() as f64 / config.rtt.to_secs()
    }

    #[test]
    fn presets_are_lossless() {
        for config in [SimpleNetworkConfig::lan(), SimpleNetworkConfig::wan()] {
            let channel = config.channel_config(inter_party_link());
            assert_eq!(channel.package_loss.value(), 0.0);
        }
    }

    /// Both presets pick a window at or above the link's bandwidth-delay product, so the nominal
    /// bandwidth — and not the window — is what sets the rate.
    #[test]
    fn presets_are_bandwidth_bound() {
        for config in [SimpleNetworkConfig::lan(), SimpleNetworkConfig::wan()] {
            let channel = config.channel_config(inter_party_link());
            assert!(window_bound(&channel) >= channel.bandwidth.value() as f64);
            assert_eq!(
                channel.lossless_throughput(),
                channel.bandwidth.value() as f64
            );
        }
    }

    /// The WAN preset is the slower link: same message, strictly longer delay.
    #[test]
    fn wan_is_slower_than_lan() {
        let link = inter_party_link();
        let lan = SimpleNetworkConfig::lan().channel_config(link);
        let wan = SimpleNetworkConfig::wan().channel_config(link);
        assert!(wan.message_delay(1 << 20) > lan.message_delay(1 << 20));
    }

    /// A party's link to itself carries no delay, whichever preset is in use.
    #[test]
    fn self_links_are_instant() {
        for config in [
            SimpleNetworkConfig::default(),
            SimpleNetworkConfig::lan(),
            SimpleNetworkConfig::wan(),
        ] {
            let channel = config.channel_config(self_link());
            assert_eq!(channel.net_type, NetworkType::Instant);
            assert_eq!(channel.message_delay(1 << 20), Duration::ZERO);
        }
    }
}
