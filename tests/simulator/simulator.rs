use scl_rs::net::simulation::channel::{
    Bandwidth, ChannelConfig, ChannelConfigBuilder, ChannelId, NetworkConfig, NetworkType, Rtt,
    SimpleNetworkConfig,
};
use scl_rs::net::simulation::event::{Event, EventType};
use scl_rs::net::simulation::hook::TriggeredHook;
use scl_rs::net::simulation::manager::Manager;
use scl_rs::net::simulation::network::SimulatedNetwork;
use scl_rs::net::simulation::simulator::simulate;
use scl_rs::net::simulation::SimulationTrace;
use scl_rs::net::{Network, Packet, PartyId};
use scl_rs::protocol::{Environment, Protocol, ProtocolResult};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct SendRecvProtocol;

#[async_trait::async_trait]
impl Protocol<SimulatedNetwork<SimpleNetworkConfig>> for SendRecvProtocol {
    async fn run(
        &self,
        environment: &mut Environment<SimulatedNetwork<SimpleNetworkConfig>>,
    ) -> ProtocolResult<SimulatedNetwork<SimpleNetworkConfig>> {
        let mut packet = Packet::empty();
        packet
            .write(&environment.network.local_party().as_usize())
            .unwrap();

        let other = environment.network.other().unwrap();
        environment.network.send_to(other, &packet).await.unwrap();

        let received_packet = environment.network.recv_from(other).await.unwrap();
        environment.network.close().await.unwrap();

        ProtocolResult::with_result_only(received_packet.bytes())
    }

    fn name(&self) -> String {
        String::from("SendRecvProtocol")
    }
}

pub struct SendRecvManager {
    parties: Vec<PartyId>,
    send_recv_net_config: SimpleNetworkConfig,
}

impl Manager<SimpleNetworkConfig, SimulatedNetwork<SimpleNetworkConfig>> for SendRecvManager {
    fn add_hook(&mut self, _: Event, _: Arc<dyn TriggeredHook<SimpleNetworkConfig>>) {}

    fn add_unconditional_hook(&mut self, _: Arc<dyn TriggeredHook<SimpleNetworkConfig>>) {}

    fn protocol(&self) -> Vec<Box<dyn Protocol<SimulatedNetwork<SimpleNetworkConfig>>>> {
        let mut protocols = Vec::new();
        for _ in self.parties.iter() {
            let protocol = SendRecvProtocol;
            protocols.push(
                Box::new(protocol) as Box<dyn Protocol<SimulatedNetwork<SimpleNetworkConfig>>>
            );
        }
        protocols
    }

    fn handle_protocol_output(&mut self, party_id: PartyId, output: Vec<u8>) {
        println!("Party {} output: {:?}", party_id.as_usize(), output);

        // Each party should receive the other party's id.
        let mut expected = Packet::empty();
        expected.write(&(1 - party_id.as_usize())).unwrap();
        assert_eq!(output, expected.bytes());
    }

    fn handle_simulator_output(&mut self, party_id: PartyId, trace: &SimulationTrace) {
        // Pretty-print the full event trace for this party (console or, with `write!`, a file).
        println!("Party {}:\n{}", party_id.as_usize(), trace);
        println!("---");

        let event_types = trace
            .events()
            .iter()
            .map(|e| e.event_type())
            .collect::<Vec<_>>();
        assert_eq!(
            event_types,
            [
                EventType::Start,
                EventType::ProtocolBegin,
                EventType::SendData,
                EventType::ReceiveData,
                EventType::CloseChannel,
                EventType::CloseChannel,
                EventType::Output,
                EventType::ProtocolEnd,
                EventType::Stop,
            ]
        )
    }

    fn network_config(&self) -> &SimpleNetworkConfig {
        &self.send_recv_net_config
    }

    fn hooks(&self) -> Vec<Arc<dyn TriggeredHook<SimpleNetworkConfig>>> {
        vec![]
    }
}

#[tokio::test]
async fn send_recv_simulation() {
    let manager = Arc::new(Mutex::new(SendRecvManager {
        parties: vec![PartyId::from(0), PartyId::from(1)],
        send_recv_net_config: SimpleNetworkConfig,
    }));
    simulate(vec![PartyId::from(0), PartyId::from(1)], manager.clone()).await;
}

/// Protocol where each party sends two ordered messages (`me*10`, then `me*10 + 1`) to the other
/// party and echoes back the two values it received, in arrival order. Used to check that the
/// transport delivers multiple messages in FIFO order.
pub struct PingPongProtocol;

#[async_trait::async_trait]
impl Protocol<SimulatedNetwork<SimpleNetworkConfig>> for PingPongProtocol {
    async fn run(
        &self,
        environment: &mut Environment<SimulatedNetwork<SimpleNetworkConfig>>,
    ) -> ProtocolResult<SimulatedNetwork<SimpleNetworkConfig>> {
        let other = environment.network.other().unwrap();
        let me = environment.network.local_party().as_usize();

        // Send two messages in order.
        for i in 0..2 {
            let mut packet = Packet::empty();
            packet.write(&(me * 10 + i)).unwrap();
            environment.network.send_to(other, &packet).await.unwrap();
        }

        // Receive both, preserving arrival order.
        let mut received = Packet::empty();
        for _ in 0..2 {
            let packet = environment.network.recv_from(other).await.unwrap();
            let value: usize = packet.read(0).unwrap();
            received.write(&value).unwrap();
        }
        environment.network.close().await.unwrap();

        ProtocolResult::with_result_only(received.bytes())
    }

    fn name(&self) -> String {
        String::from("PingPongProtocol")
    }
}

pub struct PingPongManager {
    parties: Vec<PartyId>,
    net_config: SimpleNetworkConfig,
}

impl Manager<SimpleNetworkConfig, SimulatedNetwork<SimpleNetworkConfig>> for PingPongManager {
    fn add_hook(&mut self, _: Event, _: Arc<dyn TriggeredHook<SimpleNetworkConfig>>) {}

    fn add_unconditional_hook(&mut self, _: Arc<dyn TriggeredHook<SimpleNetworkConfig>>) {}

    fn protocol(&self) -> Vec<Box<dyn Protocol<SimulatedNetwork<SimpleNetworkConfig>>>> {
        self.parties
            .iter()
            .map(|_| {
                Box::new(PingPongProtocol)
                    as Box<dyn Protocol<SimulatedNetwork<SimpleNetworkConfig>>>
            })
            .collect()
    }

    fn handle_protocol_output(&mut self, party_id: PartyId, output: Vec<u8>) {
        // The other party sends `other*10` then `other*10 + 1`, in that order.
        let other = 1 - party_id.as_usize();
        let mut expected = Packet::empty();
        expected.write(&(other * 10)).unwrap();
        expected.write(&(other * 10 + 1)).unwrap();
        assert_eq!(output, expected.bytes());
    }

    fn handle_simulator_output(&mut self, _: PartyId, _: &SimulationTrace) {}

    fn network_config(&self) -> &SimpleNetworkConfig {
        &self.net_config
    }

    fn hooks(&self) -> Vec<Arc<dyn TriggeredHook<SimpleNetworkConfig>>> {
        vec![]
    }
}

#[tokio::test]
async fn ping_pong_preserves_message_order() {
    let manager = Arc::new(Mutex::new(PingPongManager {
        parties: vec![PartyId::from(0), PartyId::from(1)],
        net_config: SimpleNetworkConfig,
    }));
    simulate(vec![PartyId::from(0), PartyId::from(1)], manager.clone()).await;
}

/// First stage of a chained protocol. It exchanges party ids over the network and then hands off
/// to a second stage, carrying the received value inside the next protocol. Used to check that
/// `ProtocolResult::next_protocol` chaining runs and that state flows between stages.
pub struct ChainedFirstStage;

#[async_trait::async_trait]
impl Protocol<SimulatedNetwork<SimpleNetworkConfig>> for ChainedFirstStage {
    async fn run(
        &self,
        environment: &mut Environment<SimulatedNetwork<SimpleNetworkConfig>>,
    ) -> ProtocolResult<SimulatedNetwork<SimpleNetworkConfig>> {
        let other = environment.network.other().unwrap();
        let me = environment.network.local_party().as_usize();

        let mut packet = Packet::empty();
        packet.write(&me).unwrap();
        environment.network.send_to(other, &packet).await.unwrap();

        let received: usize = environment
            .network
            .recv_from(other)
            .await
            .unwrap()
            .read(0)
            .unwrap();
        environment.network.close().await.unwrap();

        // No result here: the chain continues with the value baked into the next stage.
        ProtocolResult::with_next(Box::new(ChainedSecondStage { received }))
    }

    fn name(&self) -> String {
        String::from("ChainedFirstStage")
    }
}

/// Second stage of the chained protocol. It carries the value received in the first stage and
/// emits `received + 100` as the final protocol result, without using the network.
pub struct ChainedSecondStage {
    received: usize,
}

#[async_trait::async_trait]
impl Protocol<SimulatedNetwork<SimpleNetworkConfig>> for ChainedSecondStage {
    async fn run(
        &self,
        _environment: &mut Environment<SimulatedNetwork<SimpleNetworkConfig>>,
    ) -> ProtocolResult<SimulatedNetwork<SimpleNetworkConfig>> {
        let mut packet = Packet::empty();
        packet.write(&(self.received + 100)).unwrap();
        ProtocolResult::with_result_only(packet.bytes())
    }

    fn name(&self) -> String {
        String::from("ChainedSecondStage")
    }
}

pub struct ChainedManager {
    parties: Vec<PartyId>,
    net_config: SimpleNetworkConfig,
}

impl Manager<SimpleNetworkConfig, SimulatedNetwork<SimpleNetworkConfig>> for ChainedManager {
    fn add_hook(&mut self, _: Event, _: Arc<dyn TriggeredHook<SimpleNetworkConfig>>) {}

    fn add_unconditional_hook(&mut self, _: Arc<dyn TriggeredHook<SimpleNetworkConfig>>) {}

    fn protocol(&self) -> Vec<Box<dyn Protocol<SimulatedNetwork<SimpleNetworkConfig>>>> {
        self.parties
            .iter()
            .map(|_| {
                Box::new(ChainedFirstStage)
                    as Box<dyn Protocol<SimulatedNetwork<SimpleNetworkConfig>>>
            })
            .collect()
    }

    fn handle_protocol_output(&mut self, party_id: PartyId, output: Vec<u8>) {
        // Each party receives the other party's id in stage one, then outputs `received + 100`.
        let other = 1 - party_id.as_usize();
        let mut expected = Packet::empty();
        expected.write(&(other + 100)).unwrap();
        assert_eq!(output, expected.bytes());
    }

    fn handle_simulator_output(&mut self, _: PartyId, _: &SimulationTrace) {}

    fn network_config(&self) -> &SimpleNetworkConfig {
        &self.net_config
    }

    fn hooks(&self) -> Vec<Arc<dyn TriggeredHook<SimpleNetworkConfig>>> {
        vec![]
    }
}

#[tokio::test]
async fn chained_protocols_pass_state_between_stages() {
    let manager = Arc::new(Mutex::new(ChainedManager {
        parties: vec![PartyId::from(0), PartyId::from(1)],
        net_config: SimpleNetworkConfig,
    }));
    simulate(vec![PartyId::from(0), PartyId::from(1)], manager.clone()).await;
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
impl Protocol<SimulatedNetwork<SlowNetworkConfig>> for BulkTransferProtocol {
    async fn run(
        &self,
        environment: &mut Environment<SimulatedNetwork<SlowNetworkConfig>>,
    ) -> ProtocolResult<SimulatedNetwork<SlowNetworkConfig>> {
        let other = environment.network.other().unwrap();

        let mut packet = Packet::empty();
        packet.write(&vec![0u8; BULK_PAYLOAD_LEN]).unwrap();
        environment.network.send_to(other, &packet).await.unwrap();

        let received = environment.network.recv_from(other).await.unwrap();
        environment.network.close().await.unwrap();

        ProtocolResult::with_result_only(received.bytes())
    }

    fn name(&self) -> String {
        String::from("BulkTransferProtocol")
    }
}

pub struct BulkTransferManager {
    parties: Vec<PartyId>,
    net_config: SlowNetworkConfig,
}

impl Manager<SlowNetworkConfig, SimulatedNetwork<SlowNetworkConfig>> for BulkTransferManager {
    fn add_hook(&mut self, _: Event, _: Arc<dyn TriggeredHook<SlowNetworkConfig>>) {}

    fn add_unconditional_hook(&mut self, _: Arc<dyn TriggeredHook<SlowNetworkConfig>>) {}

    fn protocol(&self) -> Vec<Box<dyn Protocol<SimulatedNetwork<SlowNetworkConfig>>>> {
        self.parties
            .iter()
            .map(|_| {
                Box::new(BulkTransferProtocol)
                    as Box<dyn Protocol<SimulatedNetwork<SlowNetworkConfig>>>
            })
            .collect()
    }

    fn handle_protocol_output(&mut self, _party_id: PartyId, _output: Vec<u8>) {}

    fn handle_simulator_output(&mut self, party_id: PartyId, trace: &SimulationTrace) {
        println!("Party {}:\n{}\n---", party_id.as_usize(), trace);

        let recv_event = trace
            .events()
            .iter()
            .find(|event| event.event_type() == EventType::ReceiveData)
            .expect("the trace should contain a ReceiveData event");
        let recv_secs = recv_event.timestamp().as_secs_f64();

        // The configured latency alone contributes the 500 ms RTT.
        assert!(
            recv_secs >= 0.5,
            "party {}: reception time {recv_secs}s should reflect the 500ms RTT",
            party_id.as_usize()
        );
        // The low bandwidth over a ~20 KB payload adds several extra seconds, so the total is well
        // above the RTT-only floor. This confirms the bandwidth term is taken into account.
        assert!(
            recv_secs > 1.0,
            "party {}: reception time {recv_secs}s should reflect the bandwidth-limited transfer",
            party_id.as_usize()
        );
    }

    fn network_config(&self) -> &SlowNetworkConfig {
        &self.net_config
    }

    fn hooks(&self) -> Vec<Arc<dyn TriggeredHook<SlowNetworkConfig>>> {
        vec![]
    }
}

#[tokio::test]
async fn simulation_reflects_bandwidth_and_latency() {
    let manager = Arc::new(Mutex::new(BulkTransferManager {
        parties: vec![PartyId::from(0), PartyId::from(1)],
        net_config: SlowNetworkConfig,
    }));
    simulate(vec![PartyId::from(0), PartyId::from(1)], manager.clone()).await;
}

#[test]
fn simulation_trace_renders_events_nicely() {
    use scl_rs::net::simulation::channel::ChannelId;
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
impl Protocol<SimulatedNetwork<SimpleNetworkConfig>> for OneWayProtocol {
    async fn run(
        &self,
        environment: &mut Environment<SimulatedNetwork<SimpleNetworkConfig>>,
    ) -> ProtocolResult<SimulatedNetwork<SimpleNetworkConfig>> {
        let other = environment.network.other().unwrap();
        if environment.network.local_party().as_usize() == 0 {
            let mut packet = Packet::empty();
            packet.write(&42usize).unwrap();
            environment.network.send_to(other, &packet).await.unwrap();
            environment.network.close().await.unwrap();
            ProtocolResult::empty()
        } else {
            // Party 1 receives without ever having sent on this channel.
            let packet = environment.network.recv_from(other).await.unwrap();
            environment.network.close().await.unwrap();
            ProtocolResult::with_result_only(packet.bytes())
        }
    }

    fn name(&self) -> String {
        String::from("OneWayProtocol")
    }
}

pub struct OneWayManager {
    parties: Vec<PartyId>,
    net_config: SimpleNetworkConfig,
}

impl Manager<SimpleNetworkConfig, SimulatedNetwork<SimpleNetworkConfig>> for OneWayManager {
    fn add_hook(&mut self, _: Event, _: Arc<dyn TriggeredHook<SimpleNetworkConfig>>) {}

    fn add_unconditional_hook(&mut self, _: Arc<dyn TriggeredHook<SimpleNetworkConfig>>) {}

    fn protocol(&self) -> Vec<Box<dyn Protocol<SimulatedNetwork<SimpleNetworkConfig>>>> {
        self.parties
            .iter()
            .map(|_| {
                Box::new(OneWayProtocol) as Box<dyn Protocol<SimulatedNetwork<SimpleNetworkConfig>>>
            })
            .collect()
    }

    fn handle_protocol_output(&mut self, party_id: PartyId, output: Vec<u8>) {
        // Only party 1 produces an output: the value 42 sent by party 0.
        assert_eq!(party_id.as_usize(), 1);
        let mut expected = Packet::empty();
        expected.write(&42usize).unwrap();
        assert_eq!(output, expected.bytes());
    }

    fn handle_simulator_output(&mut self, _: PartyId, _: &SimulationTrace) {}

    fn network_config(&self) -> &SimpleNetworkConfig {
        &self.net_config
    }

    fn hooks(&self) -> Vec<Arc<dyn TriggeredHook<SimpleNetworkConfig>>> {
        vec![]
    }
}

#[tokio::test]
async fn one_way_communication_does_not_require_prior_send() {
    let manager = Arc::new(Mutex::new(OneWayManager {
        parties: vec![PartyId::from(0), PartyId::from(1)],
        net_config: SimpleNetworkConfig,
    }));
    simulate(vec![PartyId::from(0), PartyId::from(1)], manager.clone()).await;
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
