use crate::net::simulation::channel::NetworkConfig;
use crate::net::simulation::event::Event;
use crate::net::PartyId;

pub trait Hook<N: NetworkConfig>: Send + Sync {
    fn run(&self, party_id: PartyId, context: &mut N);
}

pub struct TriggeredHook<N: NetworkConfig> {
    /// Trigger of the hook.
    ///
    /// This trigger could be optional meaning that the Hook could be executed unconditionally.
    trigger: Option<Event>,
    /// Hook that will be executed after the trigger is dispatched.
    hook: Box<dyn Hook<N>>,
}

impl<N> TriggeredHook<N>
where
    N: NetworkConfig,
{
    /// Creates a new trigger of the hook.
    pub fn new(trigger: Option<Event>, hook: Box<dyn Hook<N>>) -> Self {
        Self { trigger, hook }
    }
}
