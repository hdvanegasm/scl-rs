use scl_rs::net::simulation::channel::{
    Bandwidth, ChannelConfig, ChannelConfigBuilder, Link, NetworkConfig, NetworkType, Rtt,
    SimpleNetworkConfig,
};
use scl_rs::net::simulation::event::{Event, EventType};
use scl_rs::net::simulation::network::SimNetwork;
use scl_rs::net::simulation::runtime::simulate;
use scl_rs::net::simulation::switchboard::{Switchboard, TriggeredHook};
use scl_rs::net::simulation::SimulationTrace;
use scl_rs::net::{Network, Packet, PartyId};
use scl_rs::protocol::{Error, GeneralEnv, Protocol};

pub struct SendRecvProtocol;

#[async_trait::async_trait]
impl Protocol<GeneralEnv<SimNetwork>> for SendRecvProtocol {
    type Output = usize;

    async fn run(self, environment: &mut GeneralEnv<SimNetwork>) -> Result<usize, Error> {
        let mut packet = Packet::empty();
        packet
            .write(&environment.network.local_party().as_usize())
            .unwrap();

        let other = environment.network.other()?;
        environment.network.send_to(other, &packet).await?;

        let received_packet = environment.network.recv_from(other).await?;
        environment.network.close().await?;

        let their_id: usize = received_packet.read(0).unwrap();
        Ok(their_id)
    }

    fn name(&self) -> &'static str {
        "SendRecvProtocol"
    }
}

#[test]
fn send_recv_simulation() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| SendRecvProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    // Each party receives the other party's id.
    for party in [p0, p1] {
        assert_eq!(outcome.outputs[&party], 1 - party.as_usize());
    }

    // The event-loop model has no per-connection channels to close, so there are no
    // `CloseChannel` events (the old transport recorded two).
    assert_eq!(
        outcome.traces[&p0].event_types(),
        vec![
            EventType::Start,
            EventType::ProtocolBegin,
            EventType::SendData,
            EventType::ReceiveData,
            EventType::ProtocolEnd,
            EventType::Output,
            EventType::Stop,
        ]
    );
}

/// Protocol where each party sends two ordered messages (`me*10`, then `me*10 + 1`) to the other
/// party and echoes back the two values it received, in arrival order. Used to check that the
/// transport delivers multiple messages in FIFO order.
pub struct PingPongProtocol;

#[async_trait::async_trait]
impl Protocol<GeneralEnv<SimNetwork>> for PingPongProtocol {
    type Output = Vec<usize>;

    async fn run(self, environment: &mut GeneralEnv<SimNetwork>) -> Result<Vec<usize>, Error> {
        let other = environment.network.other()?;
        let me = environment.network.local_party().as_usize();

        // Send two messages in order.
        for i in 0..2 {
            let mut packet = Packet::empty();
            packet.write(&(me * 10 + i)).unwrap();
            environment.network.send_to(other, &packet).await?;
        }

        // Receive both, preserving arrival order.
        let mut received = Vec::new();
        for _ in 0..2 {
            let packet = environment.network.recv_from(other).await?;
            let value: usize = packet.read(0).unwrap();
            received.push(value);
        }
        environment.network.close().await?;

        Ok(received)
    }

    fn name(&self) -> &'static str {
        "PingPongProtocol"
    }
}

#[test]
fn ping_pong_preserves_message_order() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| PingPongProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    // The other party sends `other*10` then `other*10 + 1`, in that order.
    for party in [p0, p1] {
        let other = 1 - party.as_usize();
        assert_eq!(outcome.outputs[&party], vec![other * 10, other * 10 + 1]);
    }
}

/// First stage of a chained protocol. It exchanges party ids over the network and then calls a
/// second-stage protocol inline, using its typed result as its own output. Used to check that a
/// protocol can call another protocol and consume its typed return directly.
pub struct ChainedFirstStage;

#[async_trait::async_trait]
impl Protocol<GeneralEnv<SimNetwork>> for ChainedFirstStage {
    type Output = usize;

    async fn run(self, environment: &mut GeneralEnv<SimNetwork>) -> Result<usize, Error> {
        let other = environment.network.other()?;
        let me = environment.network.local_party().as_usize();

        let mut packet = Packet::empty();
        packet.write(&me).unwrap();
        environment.network.send_to(other, &packet).await?;

        let received: usize = environment.network.recv_from(other).await?.read(0).unwrap();
        environment.network.close().await?;

        // Composition: call the next stage inline and use its typed result.
        let output = ChainedSecondStage { received }.execute(environment).await?;
        Ok(output)
    }

    fn name(&self) -> &'static str {
        "ChainedFirstStage"
    }
}

/// Second stage of the chained protocol. It carries the value received in the first stage and
/// returns `received + 100`, without using the network.
pub struct ChainedSecondStage {
    received: usize,
}

#[async_trait::async_trait]
impl Protocol<GeneralEnv<SimNetwork>> for ChainedSecondStage {
    type Output = usize;

    async fn run(self, _environment: &mut GeneralEnv<SimNetwork>) -> Result<usize, Error> {
        Ok(self.received + 100)
    }

    fn name(&self) -> &'static str {
        "ChainedSecondStage"
    }
}

#[test]
fn chained_protocols_pass_state_between_stages() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| ChainedFirstStage,
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    // Each party receives the other party's id in stage one, then outputs `received + 100`.
    for party in [p0, p1] {
        let other = 1 - party.as_usize();
        assert_eq!(outcome.outputs[&party], other + 100);
    }
}

/// Network configuration with a non-trivial, bandwidth-limited and high-latency cross-party
/// channel (50_000 bits/s, 500 ms RTT). The loopback channel is kept instantaneous.
#[derive(Debug, Clone)]
pub struct SlowNetworkConfig;

impl NetworkConfig for SlowNetworkConfig {
    fn channel_config(&self, link: Link) -> ChannelConfig {
        if link.sender() == link.recipient() {
            // SAFETY: default builder values are valid, so the build does not fail.
            return ChannelConfigBuilder::default()
                .net_type(NetworkType::Instant)
                .build()
                .unwrap();
        }
        // SAFETY: the values below are valid, so the build does not fail.
        ChannelConfigBuilder::default()
            .net_type(NetworkType::Tcp)
            .bandwidth(Bandwidth::new(50_000))
            .rtt(Rtt::new(500))
            .build()
            .unwrap()
    }
}

/// Length in bytes of the payload exchanged in the bulk-transfer protocol. It is large enough that,
/// at the configured bandwidth, the transmission time dominates over the RTT.
const BULK_PAYLOAD_LEN: usize = 200_000;

/// Protocol where each party sends a large payload to the other and waits for the other's payload.
/// Used to observe how bandwidth and latency affect the simulated reception time.
pub struct BulkTransferProtocol;

#[async_trait::async_trait]
impl Protocol<GeneralEnv<SimNetwork>> for BulkTransferProtocol {
    type Output = Vec<u8>;

    async fn run(self, environment: &mut GeneralEnv<SimNetwork>) -> Result<Vec<u8>, Error> {
        let other = environment.network.other()?;

        let mut packet = Packet::empty();
        packet.write(&vec![0u8; BULK_PAYLOAD_LEN]).unwrap();
        environment.network.send_to(other, &packet).await?;

        let received = environment.network.recv_from(other).await?;
        environment.network.close().await?;

        let payload: Vec<u8> = received.read(0).unwrap();
        Ok(payload)
    }

    fn name(&self) -> &'static str {
        "BulkTransferProtocol"
    }
}

#[test]
fn simulation_reflects_bandwidth_and_latency() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SlowNetworkConfig,
        vec![p0, p1],
        |_| BulkTransferProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    for party in [p0, p1] {
        let recv_event = outcome.traces[&party]
            .events()
            .iter()
            .find(|event| event.event_type() == EventType::ReceiveData)
            .expect("the trace should contain a ReceiveData event");
        let recv_secs = recv_event.timestamp().as_secs_f64();

        // The configured latency alone contributes the 500 ms RTT.
        assert!(
            recv_secs >= 0.5,
            "party {}: reception time {recv_secs}s should reflect the 500ms RTT",
            party.as_usize()
        );
        // The low bandwidth over a large payload adds several extra seconds, so the total is well
        // above the RTT-only floor. This confirms the bandwidth term is taken into account.
        assert!(
            recv_secs > 1.0,
            "party {}: reception time {recv_secs}s should reflect the bandwidth-limited transfer",
            party.as_usize()
        );
        println!(
            "---- Party {}:\n{}",
            party.as_usize(),
            outcome.traces[&party]
        );
    }
}

#[test]
fn simulation_trace_renders_events_nicely() {
    use std::time::Duration;

    // A directed link carrying packets from party 0 to party 1.
    let channel = Link::new(PartyId::from(0), PartyId::from(1));
    let trace = SimulationTrace::new(vec![
        Event::Start {
            timestamp: Duration::ZERO,
        },
        Event::SendData {
            timestamp: Duration::from_millis(100),
            link: channel,
            size: 8,
        },
        Event::ReceiveData {
            timestamp: Duration::from_millis(200),
            link: channel,
            size: 16,
        },
        Event::Stop {
            timestamp: Duration::from_millis(200),
        },
    ]);

    let rendered = trace.to_string();

    // One line per event, in order.
    assert_eq!(rendered.lines().count(), 4);
    // Each event is rendered from the recording party's perspective: a send on the 0 -> 1 link is
    // shown as `0 -> 1`, while a receive on that same link is the receiver's view, `1 <- 0`.
    assert!(rendered.contains("START"));
    assert!(rendered.contains("SEND"));
    assert!(rendered.contains("0 -> 1 (8 bytes)"));
    assert!(rendered.contains("RECV"));
    assert!(rendered.contains("1 <- 0 (16 bytes)"));
    assert!(rendered.contains("STOP"));
}

#[test]
fn trace_arrows_reflect_each_party_perspective() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| SendRecvProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    // Party 0's perspective: send to party 1, receive from party 1.
    let trace0 = outcome.traces[&p0].to_string();
    assert!(
        trace0.contains("0 -> 1"),
        "party 0 should send to party 1:\n{trace0}"
    );
    assert!(
        trace0.contains("0 <- 1"),
        "party 0 should receive from party 1:\n{trace0}"
    );

    // Party 1's perspective (the higher id, where the canonical-id bug surfaced): its arrows must
    // point to/from party 0 from its own viewpoint, not be rendered as `0 -> 1` / `0 <- 1`.
    let trace1 = outcome.traces[&p1].to_string();
    assert!(
        trace1.contains("1 -> 0"),
        "party 1 should send to party 0:\n{trace1}"
    );
    assert!(
        trace1.contains("1 <- 0"),
        "party 1 should receive from party 0:\n{trace1}"
    );
    assert!(
        !trace1.contains("0 -> 1") && !trace1.contains("0 <- 1"),
        "party 1's arrows must reflect its own perspective, not the canonical channel id:\n{trace1}"
    );
}

/// Protocol where party 0 only sends and party 1 only receives, with no message in the reverse
/// direction. This is a regression test for a latency-bookkeeping bug where a party could not
/// receive on a channel unless it had previously sent on it.
pub struct OneWayProtocol;

#[async_trait::async_trait]
impl Protocol<GeneralEnv<SimNetwork>> for OneWayProtocol {
    type Output = Option<usize>;

    async fn run(self, environment: &mut GeneralEnv<SimNetwork>) -> Result<Option<usize>, Error> {
        let other = environment.network.other()?;
        if environment.network.local_party().as_usize() == 0 {
            let mut packet = Packet::empty();
            packet.write(&42usize).unwrap();
            environment.network.send_to(other, &packet).await?;
            environment.network.close().await?;
            Ok(None)
        } else {
            // Party 1 receives without ever having sent on this channel.
            let packet = environment.network.recv_from(other).await?;
            environment.network.close().await?;
            let value: usize = packet.read(0).unwrap();
            Ok(Some(value))
        }
    }

    fn name(&self) -> &'static str {
        "OneWayProtocol"
    }
}

#[test]
fn one_way_communication_does_not_require_prior_send() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| OneWayProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    // Party 0 produces no output; party 1 outputs the value 42 sent by party 0.
    assert_eq!(outcome.outputs[&p0], None);
    assert_eq!(outcome.outputs[&p1], Some(42usize));
}

/// Number of parties in the broadcast simulation.
const BROADCAST_N_PARTIES: usize = 5;

/// Value that party 0 broadcasts to every party.
const BROADCAST_VALUE: usize = 7;

/// Protocol where party 0 sends the same message to every party (itself included) and every party
/// receives its copy from party 0, after which the protocol finishes. Used to exercise a
/// multi-party (5 parties) simulation with a single sender fanning out to all receivers.
pub struct BroadcastProtocol;

#[async_trait::async_trait]
impl Protocol<GeneralEnv<SimNetwork>> for BroadcastProtocol {
    type Output = usize;

    async fn run(self, environment: &mut GeneralEnv<SimNetwork>) -> Result<usize, Error> {
        let me = environment.network.local_party();

        if me.as_usize() == 0 {
            // Party 0 sends the same message to every party, including itself.
            let mut packet = Packet::empty();
            packet.write(&BROADCAST_VALUE).unwrap();
            for receiver in 0..BROADCAST_N_PARTIES {
                environment
                    .network
                    .send_to(PartyId::from(receiver), &packet)
                    .await?;
            }
        }

        // Every party (party 0 included) receives its copy from party 0.
        let received = environment
            .network
            .recv_from(PartyId::from(0_usize))
            .await?;
        environment.network.close().await?;

        let value: usize = received.read(0).unwrap();
        Ok(value)
    }

    fn name(&self) -> &'static str {
        "BroadcastProtocol"
    }
}

#[test]
fn broadcast_from_party_zero_reaches_all_parties() {
    let parties: Vec<PartyId> = (0..BROADCAST_N_PARTIES).map(PartyId::from).collect();
    let outcome = simulate(
        SimpleNetworkConfig,
        parties.clone(),
        |_| BroadcastProtocol,
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    for party in &parties {
        // Every party outputs the value broadcast by party 0.
        assert_eq!(outcome.outputs[party], BROADCAST_VALUE);

        let trace = &outcome.traces[party];
        let sends = trace
            .events()
            .iter()
            .filter(|event| event.event_type() == EventType::SendData)
            .count();
        let recvs = trace
            .events()
            .iter()
            .filter(|event| event.event_type() == EventType::ReceiveData)
            .count();

        if party.as_usize() == 0 {
            // Party 0 sends one message to each of the parties (itself included).
            assert_eq!(sends, BROADCAST_N_PARTIES);
        } else {
            // The other parties only receive; they never send.
            assert_eq!(sends, 0);
        }
        // Every party receives exactly one message from party 0.
        assert_eq!(recvs, 1);

        println!(
            "---- Party {}:\n{}",
            party.as_usize(),
            outcome.traces[party]
        );
    }
}

#[test]
fn output_event_elides_large_payloads() {
    use std::time::Duration;

    // Small payloads are shown in full.
    let small = SimulationTrace::new(vec![Event::Output {
        timestamp: Duration::ZERO,
        output: vec![1, 2, 3],
    }]);
    let small_rendered = small.to_string();
    assert!(small_rendered.contains("[1, 2, 3]"));
    assert!(!small_rendered.contains("more bytes"));

    // Large payloads show the first and last few bytes plus the total length.
    let large = SimulationTrace::new(vec![Event::Output {
        timestamp: Duration::ZERO,
        output: (0u8..100).collect(),
    }]);
    let large_rendered = large.to_string();
    assert!(large_rendered.contains("OUTPUT"));
    assert!(large_rendered.contains("[0, 1, 2, 3, …, 96, 97, 98, 99] (100 bytes)"));
}

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

struct SendRecv;

#[async_trait]
impl Protocol<GeneralEnv<SimNetwork>> for SendRecv {
    type Output = usize;

    async fn run(self, env: &mut GeneralEnv<SimNetwork>) -> Result<usize, Error> {
        let other = env.network.other()?;
        let me = env.network.local_party();

        let mut packet = Packet::empty();
        packet.write(&me.as_usize()).unwrap();
        env.network.send_to(other, &packet).await?;

        let received = env.network.recv_from(other).await?;
        let their_id_received: usize = received.read(0).unwrap();

        Ok(their_id_received)
    }

    fn name(&self) -> &'static str {
        "SendRecv"
    }
}

#[test]
fn real_protocol_runs_on_deterministic_core() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let outcome = simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| SendRecv,
        |_, net| GeneralEnv::new(net),
        vec![],
    );
    assert_eq!(outcome.outputs[&p0], 1_usize);
    assert_eq!(outcome.outputs[&p1], 0_usize);

    assert_eq!(
        outcome.traces[&p0].event_types(),
        vec![
            EventType::Start,
            EventType::ProtocolBegin,
            EventType::SendData,
            EventType::ReceiveData,
            EventType::ProtocolEnd,
            EventType::Output,
            EventType::Stop
        ],
    );

    println!("------ Party 0:\n{}", outcome.traces[&p0]);
    println!("------ Party 1:\n{}", outcome.traces[&p1]);
}

struct CountSendData(Arc<Mutex<usize>>);

impl TriggeredHook for CountSendData {
    fn trigger(&self) -> Option<EventType> {
        Some(EventType::SendData)
    }

    fn run(&self, _party: PartyId, _event: &Event, _switchboard: &mut Switchboard) {
        *self.0.lock().expect("lock free") += 1;
    }
}

#[test]
fn hook_fires_on_matching_event() {
    let p0 = PartyId::from(0_usize);
    let p1 = PartyId::from(1_usize);
    let count = Arc::new(Mutex::new(0_usize));
    let hook = Arc::new(CountSendData(count.clone()));

    simulate(
        SimpleNetworkConfig,
        vec![p0, p1],
        |_| SendRecv,
        |_, net| GeneralEnv::new(net),
        vec![hook],
    );

    // Each party sends exactly once → two SendData events.
    assert_eq!(*count.lock().unwrap(), 2);
}

/// Protocol exercising [`Network::recv_any`]. One `collector` party waits for `quorum`
/// messages from *whichever* peers respond first, never naming a sender in advance; the
/// parties in `senders` each send their own id to the collector, and every other party
/// stays silent. This is the quorum-wait at the heart of reliable broadcast: collect the
/// first `k`-of-`n` without blocking on the parties that never send.
struct QuorumCollect {
    /// Party that gathers messages via `recv_any`.
    collector: usize,
    /// Number of messages the collector waits for.
    quorum: usize,
    /// Parties that send their id to the collector. Parties not listed stay silent.
    senders: Vec<usize>,
}

#[async_trait]
impl Protocol<GeneralEnv<SimNetwork>> for QuorumCollect {
    /// For the collector: the sorted ids it heard from. For everyone else: empty.
    type Output = Vec<usize>;

    async fn run(self, env: &mut GeneralEnv<SimNetwork>) -> Result<Vec<usize>, Error> {
        let me = env.network.local_party().as_usize();

        if me == self.collector {
            let mut heard = Vec::new();
            for _ in 0..self.quorum {
                let (packet, sender) = env.network.recv_any().await?;
                // Each sender writes its own id as the payload, so the payload must match
                // the `PartyId` that `recv_any` reports alongside it.
                let payload: usize = packet.read(0).unwrap();
                assert_eq!(
                    payload,
                    sender.as_usize(),
                    "recv_any returned a mismatched (packet, sender) pair"
                );
                heard.push(sender.as_usize());
            }
            env.network.close().await?;
            heard.sort_unstable();
            Ok(heard)
        } else if self.senders.contains(&me) {
            let mut packet = Packet::empty();
            packet.write(&me).unwrap();
            env.network
                .send_to(PartyId::from(self.collector), &packet)
                .await?;
            env.network.close().await?;
            Ok(Vec::new())
        } else {
            // A silent party: it never sends, but must still terminate cleanly.
            env.network.close().await?;
            Ok(Vec::new())
        }
    }

    fn name(&self) -> &'static str {
        "QuorumCollect"
    }
}

#[test]
fn recv_any_collects_a_quorum_without_naming_senders() {
    // Five parties: party 0 collects, parties 1..=3 send, party 4 stays silent.
    let parties: Vec<PartyId> = (0..5).map(PartyId::from).collect();
    let outcome = simulate(
        SimpleNetworkConfig,
        parties.clone(),
        |_| QuorumCollect {
            collector: 0,
            quorum: 3,
            senders: vec![1, 2, 3],
        },
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    // The collector heard from exactly the three senders, even though it never named them
    // and party 4 never sent a thing: `recv_any` waits on *any* link, so the silent party
    // can neither be waited on nor cause a deadlock.
    assert_eq!(outcome.outputs[&PartyId::from(0)], vec![1, 2, 3]);
    for sender in [1, 2, 3] {
        assert_eq!(outcome.outputs[&PartyId::from(sender)], Vec::<usize>::new());
    }
    assert_eq!(outcome.outputs[&PartyId::from(4)], Vec::<usize>::new());
}

#[test]
fn recv_any_returns_at_quorum_and_does_not_wait_for_all() {
    // Five parties: party 0 collects with a quorum of 3, but parties 1..=4 all send. The
    // collector must stop after the first three and never block waiting for the fourth.
    let parties: Vec<PartyId> = (0..5).map(PartyId::from).collect();
    let outcome = simulate(
        SimpleNetworkConfig,
        parties.clone(),
        |_| QuorumCollect {
            collector: 0,
            quorum: 3,
            senders: vec![1, 2, 3, 4],
        },
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    // Exactly the quorum was collected: three distinct, valid senders, no more.
    let heard = &outcome.outputs[&PartyId::from(0)];
    assert_eq!(heard.len(), 3, "the collector should stop at the quorum");
    assert!(
        heard.iter().all(|sender| (1..=4).contains(sender)),
        "every reported sender must be one of the senders: {heard:?}"
    );
    let distinct: std::collections::HashSet<_> = heard.iter().collect();
    assert_eq!(distinct.len(), 3, "the reported senders must be distinct");

    // The collector recorded exactly `quorum` receptions, i.e. it never consumed the
    // fourth message before finishing.
    let recvs = outcome.traces[&PartyId::from(0)]
        .events()
        .iter()
        .filter(|event| event.event_type() == EventType::ReceiveData)
        .count();
    assert_eq!(recvs, 3, "the collector should receive exactly the quorum");
}

/// Party id of the straggler in [`StragglerScenario`]; also used by [`StragglerNetworkConfig`] to
/// decide which links are slow.
const STRAGGLER_ID: usize = 4;

/// Network configuration that puts the straggler on a very high-latency link (20 s RTT) while every
/// other cross-party link is fast (100 ms RTT). Loopback is instantaneous. This makes the
/// straggler's message arrive long after the collector has reached its quorum on the fast senders.
#[derive(Debug, Clone)]
pub struct StragglerNetworkConfig;

impl NetworkConfig for StragglerNetworkConfig {
    fn channel_config(&self, link: Link) -> ChannelConfig {
        if link.sender() == link.recipient() {
            // SAFETY: default builder values are valid, so the build does not fail.
            return ChannelConfigBuilder::default()
                .net_type(NetworkType::Instant)
                .build()
                .unwrap();
        }
        let touches_straggler =
            link.sender().as_usize() == STRAGGLER_ID || link.recipient().as_usize() == STRAGGLER_ID;
        let rtt_ms = if touches_straggler { 20_000 } else { 100 };
        // SAFETY: the values below are valid, so the build does not fail.
        ChannelConfigBuilder::default()
            .net_type(NetworkType::Tcp)
            .bandwidth(Bandwidth::new(1_000_000))
            .rtt(Rtt::new(rtt_ms))
            .build()
            .unwrap()
    }
}

/// Multi-role protocol for the straggler virtual-time regression. A `collector` gathers a `quorum`
/// of messages via `recv_any`; the `fast_senders` reach it quickly, while the single `straggler`
/// sits on a slow link (see [`StragglerNetworkConfig`]) so its message lands long after quorum. A
/// `late_receiver` waits *specifically* for the straggler, which keeps the simulation alive until
/// the straggler is actually delivered — so the straggler's late delivery to the collector is
/// popped *after* the collector has already finished, exercising the "delivery bumps the clock but
/// is inert once the party is done" path.
struct StragglerScenario {
    collector: PartyId,
    quorum: usize,
    fast_senders: Vec<PartyId>,
    straggler: PartyId,
    late_receiver: PartyId,
}

#[async_trait]
impl Protocol<GeneralEnv<SimNetwork>> for StragglerScenario {
    /// Collector: the sorted ids it heard. Late receiver: the straggler id it eventually got.
    /// Everyone else: empty.
    type Output = Vec<usize>;

    async fn run(self, env: &mut GeneralEnv<SimNetwork>) -> Result<Vec<usize>, Error> {
        let me = env.network.local_party();

        if me == self.collector {
            let mut heard = Vec::new();
            for _ in 0..self.quorum {
                let (_packet, sender) = env.network.recv_any().await?;
                heard.push(sender.as_usize());
            }
            env.network.close().await?;
            heard.sort_unstable();
            Ok(heard)
        } else if me == self.straggler {
            // The straggler reports its id to both the collector and the late receiver. Both links
            // are slow, so both messages arrive well after the collector's quorum.
            let mut packet = Packet::empty();
            packet.write(&me.as_usize()).unwrap();
            env.network
                .send_to(PartyId::from(self.collector), &packet)
                .await?;
            env.network
                .send_to(PartyId::from(self.late_receiver), &packet)
                .await?;
            env.network.close().await?;
            Ok(Vec::new())
        } else if self.fast_senders.contains(&me) {
            let mut packet = Packet::empty();
            packet.write(&me.as_usize()).unwrap();
            env.network
                .send_to(PartyId::from(self.collector), &packet)
                .await?;
            env.network.close().await?;
            Ok(Vec::new())
        } else if me == self.late_receiver {
            // Waits only for the straggler, keeping the simulation alive until the straggler's slow
            // messages have been delivered.
            let packet = env.network.recv_from(PartyId::from(self.straggler)).await?;
            env.network.close().await?;
            let value: usize = packet.read(0).unwrap();
            Ok(vec![value])
        } else {
            env.network.close().await?;
            Ok(Vec::new())
        }
    }

    fn name(&self) -> &'static str {
        "StragglerScenario"
    }
}

#[test]
fn straggler_delivery_after_quorum_does_not_distort_collector_time() {
    use std::time::Duration;

    // 0 collects; 1..=3 are the fast senders; 4 is the straggler on the slow link; 5 waits for the
    // straggler so the simulation outlives the collector.
    let parties: Vec<PartyId> = (0..6).map(PartyId::from).collect();
    let outcome = simulate(
        StragglerNetworkConfig,
        parties,
        |_| StragglerScenario {
            collector: PartyId::from(0),
            quorum: 3,
            fast_senders: (1..=3).map(PartyId::from).collect(),
            straggler: PartyId::from(STRAGGLER_ID),
            late_receiver: PartyId::from(5),
        },
        |_, net| GeneralEnv::new(net),
        vec![],
    );

    let collector = PartyId::from(0);
    let late_receiver = PartyId::from(5);

    // The collector reached quorum on the three fast senders; the straggler was never among them.
    assert_eq!(outcome.outputs[&collector], vec![1, 2, 3]);
    // ...yet the straggler *was* eventually delivered — the late receiver got it — so this is a
    // genuine "delivered after quorum" case, not "never delivered at all".
    assert_eq!(outcome.outputs[&late_receiver], vec![STRAGGLER_ID]);

    let collector_trace = &outcome.traces[&collector];

    // The collector consumed exactly the quorum; it never received the straggler's message.
    let recvs: Vec<&Event> = collector_trace
        .events()
        .iter()
        .filter(|event| event.event_type() == EventType::ReceiveData)
        .collect();
    assert_eq!(
        recvs.len(),
        3,
        "the collector should receive exactly the quorum"
    );

    // Virtual time at which the collector reached quorum (its last reception) and stopped.
    let quorum_time = recvs.last().unwrap().timestamp();
    let stop_time = collector_trace
        .events()
        .iter()
        .find(|event| event.event_type() == EventType::Stop)
        .expect("the collector trace should contain a Stop event")
        .timestamp();

    // The straggler's actual arrival time, observed at the late receiver.
    let straggler_arrival = outcome.traces[&late_receiver]
        .events()
        .iter()
        .find(|event| event.event_type() == EventType::ReceiveData)
        .expect("the late receiver should have received the straggler")
        .timestamp();

    // Reaching quorum and stopping happen at the same virtual instant: the post-quorum work
    // (close/output/stop) is stamped before any further delivery advances the clock.
    assert_eq!(
        stop_time, quorum_time,
        "the collector's post-quorum work must be stamped at the quorum time, not later"
    );
    // Sanity: the quorum time reflects the fast links' latency, so it is non-zero.
    assert!(quorum_time > Duration::ZERO);
    // The crux: the straggler lands far later, but that late delivery is inert for the collector —
    // it does not inflate the collector's virtual time, which stays at the fast-quorum instant.
    assert!(
        quorum_time < straggler_arrival,
        "collector quorum time {quorum_time:?} should be well before the straggler arrival {straggler_arrival:?}",
    );
}
