use crate::net::simulation::channel::NetworkConfig;
use crate::net::simulation::context::{record_event, SimulationContext};
use crate::net::simulation::event::Event;
use crate::net::simulation::manager::Manager;
use crate::net::simulation::network::{SimulatedNetwork, Transport};
use crate::net::simulation::{SimulationError, SimulationTrace};
use crate::net::PartyId;
use crate::protocol::{Environment, Protocol};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

pub struct SimulationResult {
    pub party_id: PartyId,
    pub protocol_result: Option<Vec<u8>>,
    pub trace: SimulationTrace,
}

async fn run_protocol<M, N>(
    current_party_id: PartyId,
    manager: Arc<Mutex<M>>,
    protocol: Box<dyn Protocol<SimulatedNetwork<N>>>,
    context: Arc<Mutex<SimulationContext<N>>>,
    mut env: Environment<SimulatedNetwork<N>>,
) -> Result<(), SimulationError>
where
    M: Manager<N, SimulatedNetwork<N>>,
    N: NetworkConfig,
{
    record_event(
        current_party_id,
        Event::Start {
            timestamp: Duration::ZERO,
        },
        context.clone(),
    )
    .await;

    // Protocol execution loop.
    let mut next_protocol = Some(protocol);
    while let Some(protocol) = next_protocol {
        let proto_name = protocol.name();
        {
            let last_event_timestamp = {
                let mut ctxt_guard = context.lock().await;
                ctxt_guard.start_clock(current_party_id);
                ctxt_guard.last_event_timestamp(current_party_id)?
            };
            record_event(
                current_party_id,
                Event::ProtocolBegin {
                    timestamp: last_event_timestamp,
                    protocol_name: proto_name.clone(),
                },
                context.clone(),
            )
            .await;
        }

        let result = protocol.run(&mut env).await;

        let elapsed_time = {
            let ctxt_guard = context.lock().await;
            ctxt_guard.elapsed_time_for_party(current_party_id)?
        };

        // If there is some output, handle the output and record the event.
        if let Some(result) = result.result_bytes {
            record_event(
                current_party_id,
                Event::Output {
                    timestamp: elapsed_time,
                    output: result.clone(),
                },
                context.clone(),
            )
            .await;
            let mut mngr_guard = manager.lock().await;
            mngr_guard.handle_protocol_output(current_party_id, result);
        }

        // Record the end of the protocol in the simulation context.
        let last_event_timestamp = {
            let ctxt_guard = context.lock().await;
            ctxt_guard.last_event_timestamp(current_party_id)?
        };
        record_event(
            current_party_id,
            Event::ProtocolEnd {
                timestamp: last_event_timestamp,
                protocol_name: proto_name,
            },
            context.clone(),
        )
        .await;

        next_protocol = result.next_protocol;
    }

    let last_event_timestamp = {
        let ctxt_guard = context.lock().await;
        ctxt_guard.last_event_timestamp(current_party_id)?
    };
    record_event(
        current_party_id,
        Event::Stop {
            timestamp: last_event_timestamp,
        },
        context.clone(),
    )
    .await;

    tokio::task::yield_now().await;

    Ok(())
}

pub async fn simulate<M, N>(
    party_ids: Vec<PartyId>,
    manager: Arc<Mutex<M>>,
) -> Vec<Result<(), SimulationError>>
where
    M: Manager<N, SimulatedNetwork<N>> + 'static,
    N: NetworkConfig + 'static,
{
    let (protocols, network_config, hooks) = {
        let manager_guard = manager.lock().await;
        (
            manager_guard.protocol(),
            manager_guard.network_config().clone(),
            manager_guard.hooks().clone(),
        )
    };

    let context = Arc::new(Mutex::new(SimulationContext::new(
        party_ids.clone(),
        network_config,
        hooks,
    )));

    let transport = Arc::new(Mutex::new(Transport::new(party_ids.len())));
    let networks = party_ids.iter().map(|party_id| {
        SimulatedNetwork::new(
            party_id.clone(),
            party_ids.clone(),
            transport.clone(),
            context.clone(),
        )
    });
    let envs = networks
        .into_iter()
        .map(|network| Environment::new(network));
    let mut join_set = JoinSet::new();
    for ((party_id, protocol), env) in party_ids.iter().zip(protocols).zip(envs) {
        let context = context.clone();
        let manager = manager.clone();
        join_set.spawn(run_protocol(*party_id, manager, protocol, context, env));
    }

    let result = join_set.join_all().await;

    for party_id in party_ids {
        let trace = {
            let ctxt_guard = context.lock().await;
            ctxt_guard.trace(party_id).clone()
        };
        manager
            .lock()
            .await
            .handle_simulator_output(party_id, &trace);
    }

    result
}
