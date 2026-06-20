use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;

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
    protocol::{Environment, Protocol},
};

/// The result of a [`simulate`] run: every party's output and its event trace.
pub struct SimulationOutcome<O> {
    /// The typed output produced by each party's protocol, keyed by [`PartyId`]. `O` is the
    /// protocol's [`Output`](crate::protocol::Protocol::Output) type.
    pub outputs: HashMap<PartyId, O>,
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

/// Runs every party's protocol on the deterministic core, returning each party's typed output and
/// event trace (keyed by [`PartyId`]) in a [`SimulationOutcome`].
///
/// `make_protocol` is a protocol factory closure for each party, so each party with `pid` will execute the
/// protocol `make_protocol(pid)`; `hooks` fire as events are recorded. All parties
/// run the same protocol type `P` — role differences are expressed inside the protocol (for example
/// by branching on [`local_party`](crate::net::Network::local_party)).
pub fn simulate<P, E>(
    config: impl NetworkConfig + 'static,
    parties: Vec<PartyId>,
    make_protocol: impl Fn(PartyId) -> P,
    make_env: impl Fn(PartyId, SimNetwork) -> E,
    hooks: Vec<Arc<dyn TriggeredHook>>,
) -> SimulationOutcome<P::Output>
where
    P: Protocol<E> + 'static,
    P::Output: Serialize + Send + Clone + 'static,
    E: Environment<Net = SimNetwork> + 'static,
{
    let switchboard = Arc::new(Mutex::new(Switchboard::new(ConfigDelay(config), hooks)));
    let outputs = Arc::new(Mutex::new(HashMap::new()));

    let mut tasks: Vec<Pin<Box<dyn Future<Output = ()>>>> = Vec::new();
    for party in &parties {
        let network = SimNetwork::new(*party, parties.clone(), switchboard.clone());
        let outputs = outputs.clone();
        let switchboard = switchboard.clone();
        let party = *party;
        let protocol = make_protocol(party);
        let mut env = make_env(party, network);
        tasks.push(Box::pin(async move {
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

/// Runs a party's protocol to completion, recording its lifecycle events and returning its output.
async fn drive<P, E>(
    party: PartyId,
    protocol: P,
    switchboard: Arc<Mutex<Switchboard>>,
    env: &mut E,
) -> P::Output
where
    P: Protocol<E>,
    P::Output: Serialize,
    E: Environment<Net = SimNetwork>,
{
    record_event(&switchboard, party, move |t| Event::Start { timestamp: t });
    let name = protocol.name();
    record_event(&switchboard, party, move |t| Event::ProtocolBegin {
        timestamp: t,
        protocol_name: name,
    });
    let result = protocol
        .run(env)
        .await
        .expect("the protocol should reach an end");

    record_event(&switchboard, party, move |t| Event::ProtocolEnd {
        timestamp: t,
        protocol_name: name,
    });

    let output =
        postcard::to_allocvec(&result).expect("the protocol result must serialize correctly");
    record_event(&switchboard, party, move |t| Event::Output {
        timestamp: t,
        output,
    });
    record_event(&switchboard, party, |t| Event::Stop { timestamp: t });

    result
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
