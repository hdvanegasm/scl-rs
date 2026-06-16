use scl_rs::net::simulation::channel::{
    Bandwidth, ChannelConfig, ChannelConfigBuilder, ChannelId, NetworkConfig, NetworkType, Rtt,
    SimpleNetworkConfig,
};
use scl_rs::net::simulation::event::{Event, EventType};
use scl_rs::net::simulation::network::SimNetwork;
use scl_rs::net::simulation::runtime::simulate;
use scl_rs::net::simulation::SimulationTrace;
use scl_rs::net::{Network, Packet, PartyId};
use scl_rs::protocol::{Environment, Error, Protocol};

pub struct SendRecvProtocol;

#[async_trait::async_trait]
impl Protocol<SimNetwork> for SendRecvProtocol {
    type Output = usize;

    async fn run(&self, environment: &mut Environment<SimNetwork>) -> Result<usize, Error> {
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
impl Protocol<SimNetwork> for PingPongProtocol {
    type Output = Vec<usize>;

    async fn run(&self, environment: &mut Environment<SimNetwork>) -> Result<Vec<usize>, Error> {
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
impl Protocol<SimNetwork> for ChainedFirstStage {
    type Output = usize;

    async fn run(&self, environment: &mut Environment<SimNetwork>) -> Result<usize, Error> {
        let other = environment.network.other()?;
        let me = environment.network.local_party().as_usize();

        let mut packet = Packet::empty();
        packet.write(&me).unwrap();
        environment.network.send_to(other, &packet).await?;

        let received: usize = environment.network.recv_from(other).await?.read(0).unwrap();
        environment.network.close().await?;

        // Composition: call the next stage inline and use its typed result.
        let output = ChainedSecondStage { received }.run(environment).await?;
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
impl Protocol<SimNetwork> for ChainedSecondStage {
    type Output = usize;

    async fn run(&self, _environment: &mut Environment<SimNetwork>) -> Result<usize, Error> {
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
    fn channel_config(&self, channel_id: ChannelId) -> ChannelConfig {
        if channel_id.local() == channel_id.remote() {
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
impl Protocol<SimNetwork> for BulkTransferProtocol {
    type Output = Vec<u8>;

    async fn run(&self, environment: &mut Environment<SimNetwork>) -> Result<Vec<u8>, Error> {
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

    let channel = ChannelId::new(PartyId::from(0), PartyId::from(1));
    let trace = SimulationTrace::new(vec![
        Event::Start {
            timestamp: Duration::ZERO,
        },
        Event::SendData {
            timestamp: Duration::from_millis(100),
            channel_id: channel,
            size: 8,
        },
        Event::ReceiveData {
            timestamp: Duration::from_millis(200),
            channel_id: channel,
            size: 16,
        },
        Event::Stop {
            timestamp: Duration::from_millis(200),
        },
    ]);

    let rendered = trace.to_string();

    // One line per event, in order.
    assert_eq!(rendered.lines().count(), 4);
    // Each event is rendered with its name and the expected channel direction.
    assert!(rendered.contains("START"));
    assert!(rendered.contains("SEND"));
    assert!(rendered.contains("0 -> 1 (8 bytes)"));
    assert!(rendered.contains("RECV"));
    assert!(rendered.contains("0 <- 1 (16 bytes)"));
    assert!(rendered.contains("STOP"));
}

/// Protocol where party 0 only sends and party 1 only receives, with no message in the reverse
/// direction. This is a regression test for a latency-bookkeeping bug where a party could not
/// receive on a channel unless it had previously sent on it.
pub struct OneWayProtocol;

#[async_trait::async_trait]
impl Protocol<SimNetwork> for OneWayProtocol {
    type Output = Option<usize>;

    async fn run(&self, environment: &mut Environment<SimNetwork>) -> Result<Option<usize>, Error> {
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
impl Protocol<SimNetwork> for BroadcastProtocol {
    type Output = usize;

    async fn run(&self, environment: &mut Environment<SimNetwork>) -> Result<usize, Error> {
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
            outcome.traces[&party]
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
