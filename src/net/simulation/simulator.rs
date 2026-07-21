//! Entry point that runs a protocol across all parties on the deterministic core.
//!
//! [`simulate`](crate::net::simulation::simulator::simulate) builds one shared [`Switchboard`](crate::net::simulation::switchboard::Switchboard),
//! spawns a task per party that drives its [`Protocol`](crate::protocol::Protocol) to completion over
//! a [`SimNetwork`](crate::net::simulation::network::SimNetwork), and runs them all on the
//! single-threaded executor, delivering scheduled network events in virtual-time order whenever no
//! party can make progress. It returns each party's typed output and event trace in a
//! [`SimulationOutcome`](crate::net::simulation::simulator::SimulationOutcome).

use std::{
    collections::HashMap,
    future::Future,
    io::Write,
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
            executor::run_simulation_with_idle,
            hook::TriggeredHook,
            network::SimNetwork,
            switchboard::{ConfigDelay, Switchboard},
            SimulationError, SimulationTrace,
        },
        PartyId,
    },
    prelude::ProtocolId,
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

impl<O> SimulationOutcome<O> {
    /// Reconstructs the per-protocol bandwidth call tree of `party` from its recorded trace.
    ///
    /// The tree is rebuilt post-hoc by walking the party's [`SimulationTrace`]: every
    /// [`ProtocolBegin`](Event::ProtocolBegin)/[`ProtocolEnd`](Event::ProtocolEnd) pair opens and
    /// closes a node, and each [`SendData`](Event::SendData) payload is attributed to the
    /// innermost protocol running when it was sent. The returned root is a synthetic
    /// `"<simulation>"` node spanning the whole run; bytes sent outside any protocol scope are
    /// attributed to the root itself.
    ///
    /// Only bytes *sent* by `party` are counted, as payload sizes rather than wire bytes —
    /// the same accounting as [`MetricHook`](crate::net::simulation::hook::MetricHook). Sends
    /// from a party to itself are excluded: they never touch the wire, because
    /// [`TcpNetwork`](crate::net::tcp::TcpNetwork) delivers them over an in-process loop-back
    /// channel rather than a socket.
    ///
    /// # Errors
    ///
    /// Returns [`SimulationError::PartyNotFound`] if this outcome holds no trace for `party`.
    pub fn bandwidth_tree_for(
        &self,
        party: PartyId,
    ) -> Result<ProtocolBandwidthTree, SimulationError> {
        Ok(ProtocolBandwidthTree::parse(
            ProtocolId::from("<simulation>"),
            &mut self
                .traces
                .get(&party)
                .ok_or(SimulationError::PartyNotFound(party))?
                .events(),
        ))
    }
}

/// A node of the per-protocol bandwidth call tree reconstructed from one party's trace.
///
/// Each node records the bytes the protocol sent *itself* (excluding its sub-protocols) and one
/// child per sub-protocol invocation, in call order — repeated calls of the same sub-protocol
/// appear as separate children. Build one with [`SimulationOutcome::bandwidth_tree_for`] and
/// export it with [`write_folded`](ProtocolBandwidthTree::write_folded).
pub struct ProtocolBandwidthTree {
    id: ProtocolId,
    self_bytes: usize,
    children: Vec<ProtocolBandwidthTree>,
}

impl ProtocolBandwidthTree {
    /// Builds the node for the protocol whose [`ProtocolBegin`](Event::ProtocolBegin) the cursor
    /// sits just after, consuming events through its matching [`ProtocolEnd`](Event::ProtocolEnd)
    /// (or to the end of the trace for a truncated trace or the synthetic root). Each
    /// [`SendData`](Event::SendData) is attributed to the innermost open protocol.
    fn parse(protocol_id: ProtocolId, events: &mut &[Event]) -> Self {
        let mut metric = Self {
            id: protocol_id,
            self_bytes: 0,
            children: vec![],
        };

        while let Some((first, rest)) = events.split_first() {
            *events = rest;
            match first {
                Event::SendData { link, size, .. } => {
                    // Self-sends never touch the wire (TcpNetwork loops them back in-process),
                    // so they don't count as bandwidth.
                    if link.sender() != link.recipient() {
                        metric.self_bytes += size;
                    }
                }
                Event::ProtocolBegin { protocol_name, .. } => {
                    let child = Self::parse(*protocol_name, events);
                    metric.children.push(child);
                }
                Event::ProtocolEnd { .. } => return metric,
                _ => {}
            }
        }
        metric
    }

    /// Writes this tree to `out` in the folded-stacks format understood by flamegraph tools.
    ///
    /// One line is emitted per node that sent bytes itself: the semicolon-joined protocol ids
    /// from the root down to that node, a space, and the node's own byte count. The count
    /// deliberately excludes descendants — flamegraph tools sum descendant lines into ancestor
    /// frames, so nodes that sent nothing themselves get no line yet still appear in the graph
    /// as prefixes of their descendants' paths:
    ///
    /// ```text
    /// <simulation>;Root 2
    /// <simulation>;Root;TwoRounds;DealThenOpen;PassiveDealLinearShr 30
    /// <simulation>;Root;TwoRounds;DealThenOpen;PassiveOpenLinearShr 20
    /// ```
    ///
    /// The output renders with any consumer of [Brendan Gregg's folded
    /// format](https://www.brendangregg.com/flamegraphs.html), e.g. `flamegraph.pl
    /// --countname=bytes` or `inferno-flamegraph`. Concatenating several parties' trees into one
    /// file is valid: rendering tools sum repeated paths.
    ///
    /// # Errors
    ///
    /// Propagates any I/O error raised by `out`.
    pub fn write_folded(&self, out: &mut impl Write) -> std::io::Result<()> {
        self.fold(out, &mut Vec::new())
    }

    /// Pre-order walk emitting one folded line per node with nonzero self bytes; `path` holds
    /// the ids from the root down to `self`, pushed on entry and popped symmetrically on exit.
    fn fold(&self, out: &mut impl Write, path: &mut Vec<ProtocolId>) -> std::io::Result<()> {
        path.push(self.id);
        if self.self_bytes > 0 {
            let path_joined = path
                .iter()
                .map(|node| node.to_string())
                .collect::<Vec<_>>()
                .join(";");
            out.write_all(format!("{} {}\n", path_joined, self.self_bytes).as_bytes())?;
        }

        for child in &self.children {
            child.fold(out, path)?;
        }
        path.pop();
        Ok(())
    }
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
    // A switchboard is a shared space in memory. During an execution, all the tasks may change and
    // read the swithchboard by sending and receiving messages, creating events and executing actions.
    let switchboard = Arc::new(Mutex::new(Switchboard::new(ConfigDelay(config), hooks)));
    let outputs = Arc::new(Mutex::new(HashMap::new()));

    let mut tasks: Vec<Pin<Box<dyn Future<Output = ()>>>> = Vec::new();
    for party in &parties {
        // The network contains the switchboard because receiving and sending a message must be
        // recorded.
        let network = SimNetwork::new(*party, parties.clone(), switchboard.clone());

        let outputs = outputs.clone();
        let switchboard = switchboard.clone();
        let party = *party;
        let protocol = make_protocol(party);
        let mut env = make_env(party, network);

        // Each task is: drive the current party to completion.
        tasks.push(Box::pin(async move {
            let result = drive_party_to_completion(party, protocol, switchboard, &mut env).await;
            outputs
                .lock()
                .expect("lock must be free")
                .insert(party, result);
        }));
    }

    // Run the simulation executing all the party tasks.
    run_simulation_with_idle(tasks, || {
        switchboard
            .lock()
            .expect("the mutex should be free")
            .deliver_next()
    });

    // Wrap the outputs.
    let outputs = outputs.lock().expect("lock must be free").clone();
    let traces = switchboard.lock().expect("lock must be free").take_traces();
    SimulationOutcome { outputs, traces }
}

/// Runs a party's protocol to completion, recording its lifecycle events and returning its output.
async fn drive_party_to_completion<P, E>(
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
    // `execute` brackets the protocol with protocol-begin/-end markers (recorded through the
    // network's trace hooks), exactly as it does for any sub-protocol the protocol calls — so the
    // top-level protocol nests in the trace the same way nested calls do.
    let result = protocol
        .execute(env)
        .await
        .expect("the protocol should reach an end");

    let output =
        postcard::to_allocvec(&result).expect("the protocol result must serialize correctly");
    record_event(&switchboard, party, move |t| Event::Output {
        timestamp: t,
        output,
    });
    record_event(&switchboard, party, |t| Event::Stop { timestamp: t });

    result
}
