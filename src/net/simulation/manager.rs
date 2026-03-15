use crate::net::simulation::channel::NetworkConfig;
use crate::net::simulation::event::Event;
use crate::net::simulation::hook::{Hook, TriggeredHook};
use crate::net::simulation::SimulationTrace;
use crate::net::PartyId;
use crate::protocol::Protocol;
use std::io::Write;
use std::sync::Arc;

pub trait HandleOutput {
    fn handle_simulator_output(party_id: PartyId, trace: &SimulationTrace);
    fn handle_protocol_output(party_id: PartyId, output: Vec<u8>);
}

pub trait Manager<N: NetworkConfig>: Send + Sync {
    fn add_hook(&mut self, trigger_event: Event, hook: Box<dyn Hook<N>>);
    fn add_unconditional_hook(&mut self, hook: Box<dyn Hook<N>>);
    fn protocol(&self) -> Option<Box<dyn Protocol>>;
    fn network_config(&self) -> &N;
    fn hooks(&self) -> Vec<Arc<TriggeredHook<N>>>;
}

/// Manager of a simulation with output to some stream.
///
/// The [`crate::net::simulation::Manager`] manages certain aspects of a simulation:
/// - The number of replications in the simulation.
/// - The protocol to simulate.
/// - What we do with the protocol output.
/// - What network to use.
/// - When to terminate the protocol.
/// - What to do when a protocol finishes.
pub struct IoManager<N: NetworkConfig, W: Write> {
    hooks: Vec<Arc<TriggeredHook<N>>>,
    output_stream: W,
}

impl<N, W> Manager<N> for IoManager<N, W>
where
    N: NetworkConfig,
    W: Write + Send + Sync,
{
    fn add_hook(&mut self, trigger_event: Event, hook: Box<dyn Hook<N>>) {
        self.hooks
            .push(Arc::new(TriggeredHook::new(Some(trigger_event), hook)));
    }

    fn add_unconditional_hook(&mut self, hook: Box<dyn Hook<N>>) {
        self.hooks.push(Arc::new(TriggeredHook::new(None, hook)));
    }

    fn protocol(&self) -> Option<Box<dyn Protocol>> {
        todo!()
    }

    fn network_config(&self) -> &N {
        todo!()
    }

    fn hooks(&self) -> Vec<Arc<TriggeredHook<N>>> {
        self.hooks.clone()
    }
}

impl<N, W> IoManager<N, W>
where
    N: NetworkConfig,
    W: Write,
{
    pub fn handle_simulator_output(
        &mut self,
        party_id: PartyId,
        trace: &SimulationTrace,
    ) -> Result<(), std::io::Error> {
        writeln!(self.output_stream, "Party ID: {:?}", party_id)?;
        writeln!(self.output_stream, "Simulation trace:")?;
        writeln!(self.output_stream, "{:?}", trace)?;
        Ok(())
    }
}
