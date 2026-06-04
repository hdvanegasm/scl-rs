use scl_rs::net::simulation::channel::SimpleNetworkConfig;
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
        println!(
            "Party {} trace: {:?}",
            party_id.as_usize(),
            trace.event_types()
        );
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
