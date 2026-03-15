use crate::net::simulation::channel::{NetworkConfig, SimulatedChannel};
use crate::net::simulation::context::SimulationContext;
use crate::net::simulation::manager::{IoManager, Manager};
use crate::net::simulation::transport::SimulatedNetwork;
use crate::net::simulation::SimulationError;
use crate::net::PartyId;
use crate::protocol::Protocol;
use std::sync::Arc;
use tokio::sync::Mutex;

async fn create_network() -> Arc<Mutex<SimulatedNetwork>> {
    todo!()
}

async fn run_protocols<M, N>(
    protocols: Vec<Box<dyn Protocol + 'static>>,
    context: Arc<Mutex<SimulationContext<N>>>,
    manager: Arc<Mutex<M>>,
) where
    M: Manager<N>,
    N: NetworkConfig,
{
}

async fn run_protocol<M, N>() {
    todo!()
}

pub async fn simulate<M, N>(
    party_ids: Vec<PartyId>,
    manager: Arc<Mutex<M>>,
) -> Result<(), SimulationError>
where
    M: Manager<N> + 'static,
    N: NetworkConfig + 'static,
{
    let (protocol, network_config, hooks) = {
        let manager_guard = manager.lock().await;
        (
            manager_guard.protocol(),
            manager_guard.network_config().clone(),
            manager_guard.hooks().clone(),
        )
    };
    if let Some(protocol) = protocol {
        let context = Arc::new(Mutex::new(SimulationContext::new(
            party_ids,
            network_config,
            hooks,
        )));
        tokio::spawn(run_protocols(vec![protocol], context, Arc::clone(&manager)));
    }

    Ok(())
}
