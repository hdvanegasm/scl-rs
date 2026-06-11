use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{
    net::{
        simulation::{
            channel::NetworkConfig,
            event::Event,
            executor::run_with_idle,
            network::SimNetwork,
            switchboard::{ConfigDelay, Switchboard, TriggeredHook},
            SimulationTrace,
        },
        PartyId,
    },
    protocol::Environment,
    Protocol,
};

/// The result of a [`simulate`] run: every party's output and its event trace.
pub struct SimulationOutcome {
    /// The output bytes produced by each party's protocol chain, keyed by [`PartyId`]. The value
    /// is `None` when the party's protocol finished without emitting any result bytes.
    pub outputs: HashMap<PartyId, Option<Vec<u8>>>,
    /// The time-ordered [`SimulationTrace`] recorded for each party, keyed by [`PartyId`].
    pub traces: HashMap<PartyId, SimulationTrace>,
}

/// Helper that records an event for a party at its current virtual time.
fn record_event(
    switchboard: &Arc<Mutex<Switchboard>>,
    party: PartyId,
    event_constructor: impl FnOnce(Duration) -> Event,
) {
    let mut sb_guard = switchboard.lock().expect("lock must be free");
    let timestamp = sb_guard.clock_of(party);
    sb_guard.record_event(party, event_constructor(timestamp));
}

/// Runs every party's protocol on the deterministic core, returning each party's output and event
/// trace (keyed by [`PartyId`]) in a [`SimulationOutcome`].
///
/// `protocols` pairs each party with its protocol; `hooks` fire as events are recorded.
pub fn simulate(
    config: impl NetworkConfig + 'static,
    protocols: Vec<(PartyId, Box<dyn Protocol<SimNetwork>>)>,
    hooks: Vec<Arc<dyn TriggeredHook>>,
) -> SimulationOutcome {
    let parties: Vec<PartyId> = protocols.iter().map(|(party, _)| *party).collect();
    let switchboard = Arc::new(Mutex::new(Switchboard::new(ConfigDelay(config), hooks)));
    let outputs = Arc::new(Mutex::new(HashMap::new()));

    let mut tasks: Vec<Pin<Box<dyn Future<Output = ()>>>> = Vec::new();
    for (party, protocol) in protocols {
        let network = SimNetwork::new(party, parties.clone(), switchboard.clone());
        let outputs = outputs.clone();
        let switchboard = switchboard.clone();
        tasks.push(Box::pin(async move {
            let mut env = Environment::new(network);
            let result = drive(party, protocol, switchboard, &mut env).await;
            outputs
                .lock()
                .expect("lock must be free")
                .insert(party, result);
        }));
    }

    run_simulation(switchboard.clone(), tasks);

    let outputs = outputs.lock().expect("lock must be free").clone();
    let traces = switchboard.lock().expect("lock must be free").take_traces();
    SimulationOutcome { outputs, traces }
}

/// Drives a protocol chain to the end of its execution for a party.
async fn drive(
    party: PartyId,
    protocol: Box<dyn Protocol<SimNetwork>>,
    switchboard: Arc<Mutex<Switchboard>>,
    env: &mut Environment<SimNetwork>,
) -> Option<Vec<u8>> {
    record_event(&switchboard, party, move |t| Event::Start { timestamp: t });
    let mut next_protocol = Some(protocol);
    let mut last_output = None;
    while let Some(protocol) = next_protocol {
        let name = protocol.name();
        record_event(&switchboard, party, move |t| Event::ProtocolBegin {
            timestamp: t,
            protocol_name: name,
        });
        let result = protocol.run(env).await;
        record_event(&switchboard, party, move |t| Event::ProtocolEnd {
            timestamp: t,
            protocol_name: name,
        });

        // Take the next protocol.
        if let Some(bytes) = &result.result_bytes {
            let output = bytes.clone();
            record_event(&switchboard, party, move |t| Event::Output {
                timestamp: t,
                output: output,
            });
        }
        last_output = result.result_bytes;
        next_protocol = result.next_protocol;
    }
    record_event(&switchboard, party, |t| Event::Stop { timestamp: t });
    last_output
}

/// Drives the party tasks to completion, delivering scheduled network events in
/// virtual-time order whenever no party can make progress.
fn run_simulation(
    switchboard: Arc<Mutex<Switchboard>>,
    tasks: Vec<Pin<Box<dyn Future<Output = ()>>>>,
) {
    run_with_idle(tasks, || {
        switchboard
            .lock()
            .expect("the mutex should be free")
            .deliver_next()
    });
}
