use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use scl_rs::{
    net::{
        simulation::{
            channel::SimpleNetworkConfig,
            event::{Event, EventType},
            network::SimNetwork,
            runtime::simulate,
            switchboard::{Switchboard, TriggeredHook},
        },
        Network, Packet, PartyId,
    },
    protocol::{Environment, Error},
    Protocol,
};

pub mod channel;
pub mod simulator;

struct SendRecv;

#[async_trait]
impl Protocol<SimNetwork> for SendRecv {
    type Output = usize;

    async fn run(&self, env: &mut Environment<SimNetwork>) -> Result<usize, Error> {
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
    let outcome = simulate(SimpleNetworkConfig, vec![p0, p1], |_| SendRecv, vec![]);
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

    simulate(SimpleNetworkConfig, vec![p0, p1], |_| SendRecv, vec![hook]);

    // Each party sends exactly once → two SendData events.
    assert_eq!(*count.lock().unwrap(), 2);
}
