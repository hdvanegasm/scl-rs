//! The three network regimes under comparison, and the single source of truth for both backends.
//!
//! Each [`Scenario`] is stated once here and drives *both* sides of the comparison: it builds the
//! [`ChannelConfig`] the simulator prices messages with, and it emits (via
//! [`Scenario::shell_vars`]) the `tc netem` and sysctl settings the shell driver shapes loopback
//! with. Keeping one table means a real run and a simulated run cannot silently disagree about
//! what network they are modelling.
//!
//! The three regimes are the ones the crate README reports on, and they are chosen so that a
//! *different term of the model binds* in each:
//!
//! | Scenario | Binding term | What it tests |
//! |---|---|---|
//! | [`BANDWIDTH_LIMITED`] | `bandwidth` | the serialization term, where the README reports ~0.8 % error |
//! | [`WINDOW_LIMITED`] | `window * 8 / RTT` | the window term, where the README reports the *form* holds but the default window is miscalibrated |
//! | [`LOSSY`] | `sqrt(3 / 2p)` | the loss formula, applied outside its stated validity domain |
//!
//! Which term binds is not a matter of intent — it follows from the numbers, so
//! [`Scenario::regime`] recomputes it from the parameters rather than trusting the label.

use scl_rs::net::simulation::channel::{
    Bandwidth, ChannelConfig, ChannelConfigBuilder, Link, Mss, NetworkConfig as SimNetworkConfig,
    NetworkType, PackageLoss, Rtt, WindowSize,
};

/// One network regime, with the workload sizes used to probe it.
#[derive(Debug, Clone, Copy)]
pub struct Scenario {
    /// Slug used on the command line and in the `scenario` CSV column.
    pub name: &'static str,
    /// Round-trip time, in milliseconds. Shaped as `delay rtt_ms / 2` on loopback, because a
    /// loopback packet crosses the egress qdisc once per direction.
    pub rtt_ms: usize,
    /// Link bandwidth, in bits per second.
    pub bandwidth_bps: usize,
    /// Fraction of packets dropped, in `[0, 1]`.
    pub loss: f64,
    /// TCP window the simulator prices with, in bytes.
    pub window_bytes: usize,
    /// Maximum segment size, in bytes. Loopback defaults to a 65536-byte MTU, so the driver drops
    /// it to 1500 to make this value real; the lossy path is *linearly* proportional to it.
    pub mss_bytes: usize,
    /// Number of round trips [`PingPong`](crate::protocols::PingPong) performs.
    pub ping_pong_rounds: usize,
    /// Payload [`PingPong`](crate::protocols::PingPong) carries each way, in bytes.
    pub ping_pong_payload_bytes: usize,
    /// Payload [`BulkTransfer`](crate::protocols::BulkTransfer) sends, in bytes.
    pub bulk_payload_bytes: usize,
    /// Whether the driver pins `tcp_rmem`/`tcp_wmem` to [`PINNED_TCP_MEM_BYTES`] for this
    /// scenario. Only the window-limited regime needs it; elsewhere kernel autotuning is left
    /// alone, because a large real window does not change which term binds.
    pub pin_tcp_window: bool,
}

/// Value `tcp_rmem`/`tcp_wmem` are pinned to when [`Scenario::pin_tcp_window`] is set.
///
/// Pinning it and recovering the window a real bulk transfer actually delivered gave ~70 KB — above
/// the 65,536 bytes the simulator assumes by default, and not a value any socket setting names
/// directly. Recovering that realized number is exactly what the calibration step exists for.
pub const PINNED_TCP_MEM_BYTES: usize = 131_072;

/// Bandwidth-limited and loss-less: the regime the README reports as validated.
///
/// At 1 Mbit/s over a 100 ms RTT the bandwidth-delay product is 12,500 bytes, comfortably under the
/// 65,536-byte window, so bandwidth binds and the window is inert.
pub const BANDWIDTH_LIMITED: Scenario = Scenario {
    name: "bandwidth_limited",
    rtt_ms: 100,
    bandwidth_bps: 1_000_000,
    loss: 0.0,
    window_bytes: 65_536,
    mss_bytes: 1_460,
    ping_pong_rounds: 30,
    ping_pong_payload_bytes: 1_024,
    bulk_payload_bytes: 512 * 1_024,
    pin_tcp_window: false,
};

/// Window-limited and loss-less: the regime where the model's form holds but the window needs
/// calibrating.
///
/// At 100 Mbit/s over a 100 ms RTT the bandwidth-delay product is 1,250,000 bytes, far above the
/// 65,536-byte window, so the window binds and the configured bandwidth is inert — the simulator
/// prices every message at `window * 8 / RTT`, or 5.24 Mbit/s.
pub const WINDOW_LIMITED: Scenario = Scenario {
    name: "window_limited",
    rtt_ms: 100,
    bandwidth_bps: 100_000_000,
    loss: 0.0,
    window_bytes: 65_536,
    mss_bytes: 1_460,
    ping_pong_rounds: 30,
    ping_pong_payload_bytes: 1_024,
    bulk_payload_bytes: 1_024 * 1_024,
    pin_tcp_window: true,
};

/// Lossy: the regime where a standard formula is applied outside its validity domain.
///
/// 1 % loss puts the square-root term at 1.43 Mbit/s, below the 5.24 Mbit/s window ceiling, so the
/// loss formula binds. The finding here is that real runs are not merely mispredicted but *not
/// reproducible* — 50 identical bulk-transfer trials spanned 0.73 s to 7.11 s — which is why this
/// harness repeats every measurement rather than reporting a single number.
pub const LOSSY: Scenario = Scenario {
    name: "lossy",
    rtt_ms: 100,
    bandwidth_bps: 100_000_000,
    loss: 0.01,
    window_bytes: 65_536,
    mss_bytes: 1_460,
    ping_pong_rounds: 30,
    ping_pong_payload_bytes: 1_024,
    bulk_payload_bytes: 1_024 * 1_024,
    pin_tcp_window: false,
};

/// Every scenario, in the order they are reported.
pub const ALL: [Scenario; 3] = [BANDWIDTH_LIMITED, WINDOW_LIMITED, LOSSY];

impl Scenario {
    /// Looks a scenario up by its [`name`](Scenario::name).
    pub fn by_name(name: &str) -> Option<Scenario> {
        ALL.into_iter().find(|scenario| scenario.name == name)
    }

    /// Round-trip time in seconds.
    pub fn rtt_secs(&self) -> f64 {
        self.rtt_ms as f64 / 1_000.0
    }

    /// The bandwidth-delay product in bytes: the in-flight data a link of this bandwidth and RTT
    /// can hold. The window binds exactly when this exceeds it.
    pub fn bandwidth_delay_product_bytes(&self) -> f64 {
        self.bandwidth_bps as f64 * self.rtt_secs() / 8.0
    }

    /// Recomputes which term of the model actually binds, from the parameters rather than the
    /// scenario's label — so a mis-edited table shows up as a mismatch instead of a silent change
    /// of what is being measured.
    ///
    /// Mirrors the simulator's own arithmetic: the loss-less throughput is the smaller of the
    /// bandwidth and the window rate, and a non-zero loss additionally caps it at the square-root
    /// term.
    pub fn regime(&self) -> (&'static str, f64) {
        let window_rate = 8.0 * self.window_bytes as f64 / self.rtt_secs();
        let (mut term, mut throughput) = if (self.bandwidth_bps as f64) < window_rate {
            ("bandwidth", self.bandwidth_bps as f64)
        } else {
            ("window", window_rate)
        };

        if self.loss > 0.0 {
            let lossy = f64::sqrt(3.0 / (2.0 * self.loss)) * (8.0 * self.mss_bytes as f64)
                / self.rtt_secs();
            if lossy < throughput {
                (term, throughput) = ("loss", lossy);
            }
        }

        (term, throughput)
    }

    /// Emits this scenario as `KEY=VALUE` lines for the shell driver to `eval`.
    ///
    /// The driver shapes loopback from these rather than from constants of its own, so the shaped
    /// link and the simulated link are guaranteed to describe the same network. `TC_DELAY_MS` is
    /// half the RTT: a loopback packet crosses the egress qdisc once on the way out and once on
    /// the reply, so the qdisc's per-packet delay is applied twice per round trip.
    pub fn shell_vars(&self) -> String {
        let (term, throughput) = self.regime();
        [
            format!("SCENARIO={}", self.name),
            format!("RTT_MS={}", self.rtt_ms),
            format!("BANDWIDTH_BPS={}", self.bandwidth_bps),
            format!("LOSS_FRACTION={}", self.loss),
            format!("WINDOW_BYTES={}", self.window_bytes),
            format!("MSS_BYTES={}", self.mss_bytes),
            format!("TC_DELAY_MS={}", self.rtt_ms as f64 / 2.0),
            format!("TC_RATE_BITS={}", self.bandwidth_bps),
            // Emitted at fixed precision, and paired with a boolean, so the shell never has to
            // compare a float against zero to decide whether to add netem's `loss` clause.
            format!("TC_LOSS_PERCENT={:.4}", self.loss * 100.0),
            format!("TC_HAS_LOSS={}", usize::from(self.loss > 0.0)),
            format!("TC_MTU_BYTES={}", self.mss_bytes + 40),
            format!("PIN_TCP_WINDOW={}", usize::from(self.pin_tcp_window)),
            format!("PINNED_TCP_MEM_BYTES={PINNED_TCP_MEM_BYTES}"),
            format!("BINDING_TERM={term}"),
            format!("PREDICTED_THROUGHPUT_BPS={throughput:.0}"),
        ]
        .join("\n")
    }

    /// Builds the simulator network configuration for this scenario, overriding the window with
    /// `window_bytes`.
    ///
    /// The override is what the calibration step feeds a measured window back through; passing
    /// [`Scenario::window_bytes`] reproduces the nominal configuration.
    pub fn sim_config(&self, window_bytes: usize) -> ScenarioNetwork {
        ScenarioNetwork {
            scenario: *self,
            window_bytes,
        }
    }
}

/// The simulator's [`NetworkConfig`](SimNetworkConfig) for a scenario: every inter-party link gets
/// the scenario's parameters, and a party's link to itself is instant (it never touches the wire,
/// matching how the TLS backend loops self-sends back in process).
#[derive(Debug, Clone)]
pub struct ScenarioNetwork {
    /// The scenario whose parameters every link takes.
    pub scenario: Scenario,
    /// The window to price with, which may be a calibrated value rather than the scenario's own.
    pub window_bytes: usize,
}

impl SimNetworkConfig for ScenarioNetwork {
    fn channel_config(&self, link: Link) -> ChannelConfig {
        let mut builder = ChannelConfigBuilder::default()
            .bandwidth(Bandwidth::new(self.scenario.bandwidth_bps))
            .rtt(Rtt::new(self.scenario.rtt_ms))
            .mss(Mss::new(self.scenario.mss_bytes))
            .package_loss(PackageLoss::new(self.scenario.loss))
            .window_size(WindowSize::new(self.window_bytes));

        if link.sender() == link.recipient() {
            builder = builder.net_type(NetworkType::Instant);
        }

        builder
            .build()
            .expect("scenario parameters are checked to be valid")
    }
}
