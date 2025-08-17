use crate::net::simulation::context::SimulationContext;
use crate::net::simulation::PartyId;

pub trait Hook<S: SimulationContext> {
    fn run(party_id: PartyId, context: &S);
}
