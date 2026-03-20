use scl_rs::net::simulation::channel::{
    ChannelConfig, ChannelConfigBuilder, ChannelId, NetworkConfig, SimpleNetworkConfig,
};
use scl_rs::net::simulation::context::SimulationContext;
use scl_rs::net::simulation::event::{Event, EventType};
use scl_rs::net::simulation::hook::TriggeredHook;
use scl_rs::net::simulation::manager::Manager;
use scl_rs::net::simulation::network::{SimulatedNetwork, Transport};
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
        if environment.network.local_party().as_usize() == 0 {
            packet.write(&1).unwrap();
        } else {
            packet.write(&2).unwrap();
        }

        let other: PartyId = if environment.network.local_party().as_usize() == 0 {
            PartyId::from(1)
        } else {
            PartyId::from(0)
        };

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
        println!("Party {} output: {:?}", party_id.as_usize(), output)
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
