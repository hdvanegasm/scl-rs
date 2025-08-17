use crate::net::simulation::channel::NetworkConfig;
use crate::net::simulation::PartyId;

pub trait SimulationContext {
    fn new() -> Self;
    fn trace(&self);
    fn current_time_of_party(&self, party_id: PartyId);
    fn is_party_alive(&self, party_id: PartyId) -> bool;
    fn cancel_party(&self, party_id: PartyId);
    fn cancel_simulation(&self);
}

pub struct GlobalContext<N: NetworkConfig> {
    pub number_of_parties: usize,
    pub network_config: N,
}
