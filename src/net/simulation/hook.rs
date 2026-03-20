use crate::net::simulation::channel::NetworkConfig;
use crate::net::simulation::context::SimulationContext;
use crate::net::simulation::event::EventType;
use crate::net::PartyId;
use std::sync::Arc;
use tokio::sync::Mutex;

pub trait TriggeredHook<N: NetworkConfig>: Send + Sync {
    fn trigger(&self) -> Option<EventType>;
    fn run(&self, party_id: PartyId, context: Arc<Mutex<SimulationContext<N>>>);
}
