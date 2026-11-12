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

use bytesize::ByteSize;
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
            tree::{Item, TraceTree, ROOT},
            SimulationError, SimulationTrace,
        },
        PartyId,
    },
    prelude::ProtocolId,
    protocol::{Environment, Protocol},
};

/// Flat, per-party cost totals for one simulation run: bytes sent, and time elapsed.
///
/// Built with [`SimulationOutcome::metrics`], which folds every party's [`SimulationTrace`] into
/// one byte count and one duration per party. Where [`ProtocolBandwidthTree`] breaks one party's
/// traffic down by the protocol that sent it, this is the summary view: two numbers per party, for
/// the whole run. Every party with a trace gets an entry in both, so
/// [`sent_bytes_for`](SimulationMetrics::sent_bytes_for) and
/// [`elapsed_time_for`](SimulationMetrics::elapsed_time_for) return `Some` for exactly the same
/// parties.
///
/// # Bytes
///
/// Bytes are counted as *payload* sizes rather than wire bytes — the same accounting as
/// [`MetricHook`](crate::net::simulation::hook::MetricHook) — so the framing and TLS overhead that
/// a real [`TcpNetwork`](crate::net::tcp::TcpNetwork) deployment adds is not included. Sends from a
/// party to itself are excluded, exactly as in [`SimulationOutcome::bandwidth_tree_for`]: they
/// never touch the wire, because [`TcpNetwork`](crate::net::tcp::TcpNetwork) delivers them over an
/// in-process loop-back channel rather than a socket. A party's total therefore agrees with the sum
/// over its bandwidth tree.
///
/// # Time
///
/// Elapsed time is *virtual* time on the simulator's discrete-event clock, spanning a party's first
/// recorded event to its last. It measures the modelled network — link latency and bandwidth, as
/// configured by the [`NetworkConfig`] the run was given — and nothing else: local computation is
/// free in this model, so the figure is a communication cost, not a running time you could compare
/// against a real deployment.
///
/// One consequence is worth knowing before reading these numbers. A party's clock advances only
/// when a message is *delivered to it*, so the elapsed time of a party that only ever sends stays
/// at zero however much traffic it pushed, and any party's figure really reports when its last
/// message arrived. Parties therefore finish at different virtual times, and the run as a whole
/// lasts as long as its slowest party.
///
/// Print a summary of the whole set with its [`Display`](std::fmt::Display) implementation, or read
/// one party's figures with [`sent_bytes_for`](SimulationMetrics::sent_bytes_for) and
/// [`elapsed_time_for`](SimulationMetrics::elapsed_time_for).
pub struct SimulationMetrics {
    /// Total payload bytes each party sent over the run. Every party that has a trace in the
    /// originating [`SimulationOutcome`] gets an entry, including parties that sent nothing.
    sent_bytes_party: HashMap<PartyId, ByteSize>,
    /// Virtual time each party spent in the run, from its first recorded event to its last. Keyed
    /// by the same parties as `sent_bytes_party`.
    elapsed_time_party: HashMap<PartyId, Duration>,
}

impl SimulationMetrics {
    /// Creates a metrics set that records no parties at all.
    ///
    /// This is the starting point [`SimulationOutcome::metrics`] folds the traces into; on its own
    /// it renders as `simulation metrics: <none>`.
    pub fn empty() -> Self {
        Self {
            sent_bytes_party: HashMap::new(),
            elapsed_time_party: HashMap::new(),
        }
    }

    /// Returns the total payload bytes `party` sent over the whole run.
    ///
    /// Returns `None` only when this set holds no entry for `party` — that is, when `party` took no
    /// part in the simulation these metrics came from. A party that took part but never sent
    /// anything returns `Some(ByteSize(0))`.
    pub fn sent_bytes_for(&self, party: PartyId) -> Option<ByteSize> {
        self.sent_bytes_party.get(&party).copied()
    }

    /// Returns the virtual time `party` spent in the run, from its first recorded event to its
    /// last.
    ///
    /// Returns `None` under exactly the same condition as
    /// [`sent_bytes_for`](Self::sent_bytes_for): when `party` took no part in the simulation these
    /// metrics came from. See the [type documentation](SimulationMetrics#time) for what this clock
    /// does and does not measure — in particular, a party that only sends reports
    /// `Some(Duration::ZERO)`.
    pub fn elapsed_time_for(&self, party: PartyId) -> Option<Duration> {
        self.elapsed_time_party.get(&party).copied()
    }
}

/// Renders the per-party figures as an indented table: one row per party in [`PartyId`] order,
/// then a row for the run as a whole.
///
/// ```text
/// simulation metrics:
///                     sent  elapsed
///     party 0      1.5 MiB   1.250s
///     party 1      1.5 MiB   1.250s
///     party 2    768.0 KiB   0.750s
///     whole run    3.8 MiB   1.250s
/// ```
///
/// The two columns aggregate differently on the `whole run` row, because they describe different
/// kinds of quantity: bytes add up over the parties, while their elapsed times overlap, so the run
/// lasts as long as its slowest party rather than as long as all of them put together.
///
/// Byte counts use [`ByteSize`]'s default rendering, which is IEC binary units — `KiB`, `MiB`, …,
/// and a plain `B` count below 1024 — at one decimal place. Times are seconds to three decimals,
/// matching the timestamps in an [`Event`] line. Parties are listed in id order rather than in hash
/// order, so that the output is reproducible across runs like the rest of the simulator. A set with
/// no parties in it renders as a single `simulation metrics: <none>` line.
///
/// The summary carries no trailing newline, so it can be dropped into a larger report; print it
/// with `println!("{metrics}")`.
impl std::fmt::Display for SimulationMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const INDENT: &str = "    ";
        const RUN_LABEL: &str = "whole run";
        const SENT_HEADER: &str = "sent";
        const TIME_HEADER: &str = "elapsed";

        f.write_str("simulation metrics:")?;
        if self.sent_bytes_party.is_empty() {
            return f.write_str(" <none>");
        }

        // `HashMap` iteration order varies from run to run, so the parties are put back in id order
        // to keep the rendering deterministic.
        let mut parties: Vec<_> = self.sent_bytes_party.iter().collect();
        parties.sort_unstable_by_key(|(party, _)| **party);

        // Seconds to three decimals, as an `Event` renders its timestamp.
        let render_time = |elapsed: Duration| format!("{:.3}s", elapsed.as_secs_f64());

        // The rows are rendered before they are written so every column can be padded to its widest
        // entry: `ByteSize` picks a unit per value, so the widths are only known once formatted.
        let mut rows: Vec<(String, String, String)> = parties
            .iter()
            .map(|(party, sent)| {
                // Both maps are filled together by `SimulationOutcome::metrics`, so every party
                // listed here has an elapsed time too.
                let elapsed = self
                    .elapsed_time_party
                    .get(*party)
                    .copied()
                    .unwrap_or_default();
                (
                    format!("party {}", party.as_usize()),
                    sent.to_string(),
                    render_time(elapsed),
                )
            })
            .collect();

        // Bytes sum over the parties; elapsed times overlap, so the run lasts as long as its
        // slowest party.
        let total_sent = ByteSize(parties.iter().map(|(_, sent)| sent.as_u64()).sum());
        let makespan = self
            .elapsed_time_party
            .values()
            .copied()
            .max()
            .unwrap_or_default();
        rows.push((
            RUN_LABEL.to_owned(),
            total_sent.to_string(),
            render_time(makespan),
        ));

        let label_width = rows
            .iter()
            .map(|(label, _, _)| label.len())
            .max()
            .unwrap_or(0);
        let sent_width = rows
            .iter()
            .map(|(_, sent, _)| sent.len())
            .chain([SENT_HEADER.len()])
            .max()
            .unwrap_or(0);
        let time_width = rows
            .iter()
            .map(|(_, _, elapsed)| elapsed.len())
            .chain([TIME_HEADER.len()])
            .max()
            .unwrap_or(0);

        // The header labels the two value columns; its own label cell is blank.
        write!(
            f,
            "\n{INDENT}{:label_width$}  {SENT_HEADER:>sent_width$}  {TIME_HEADER:>time_width$}",
            ""
        )?;
        for (label, sent, elapsed) in &rows {
            write!(
                f,
                "\n{INDENT}{label:<label_width$}  {sent:>sent_width$}  {elapsed:>time_width$}"
            )?;
        }
        Ok(())
    }
}

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
    /// Returns [`SimulationError::PartyNotFound`] if this outcome holds no trace for `party`, and
    /// [`SimulationError::MalformedTrace`] if the party's protocol scopes do not nest: a scope ends
    /// that is not the one open in its session, or ends in a session where no scope is open.
    pub fn bandwidth_tree_for(
        &self,
        party: PartyId,
    ) -> Result<ProtocolBandwidthTree, SimulationError> {
        let trace = self
            .traces
            .get(&party)
            .ok_or(SimulationError::PartyNotFound(party))?;
        let tree = TraceTree::build(trace.events());
        if let Some(anomaly) = tree.anomalies.first() {
            return Err(SimulationError::MalformedTrace {
                event_idx: anomaly.event_index,
                reason: anomaly.reason.clone(),
            });
        }
        Ok(ProtocolBandwidthTree::from_scope(
            &tree,
            ROOT,
            ProtocolId::from("<simulation>"),
        ))
    }

    /// Folds every party's recorded trace into flat per-party totals: bytes sent, and time spent.
    ///
    /// This is the whole-run summary that complements the per-protocol breakdown of
    /// [`bandwidth_tree_for`](Self::bandwidth_tree_for); see [`SimulationMetrics`] for exactly what
    /// the two figures do and do not count.
    pub fn metrics(&self) -> SimulationMetrics {
        let mut sim_metrics = SimulationMetrics::empty();

        for (party, trace) in &self.traces {
            // The party's own span: its first recorded event to its last. `simulate` brackets every
            // party with `Start` and `Stop`, so a recorded trace always has both ends.
            let elapsed = match (trace.events().first(), trace.events().last()) {
                (Some(first), Some(last)) => last.timestamp().saturating_sub(first.timestamp()),
                _ => Duration::ZERO,
            };
            sim_metrics.elapsed_time_party.insert(*party, elapsed);

            let entry_bytes = sim_metrics.sent_bytes_party.entry(*party).or_default();
            for event in trace.events() {
                if let Event::SendData { link, size, .. } = event {
                    // Self-sends never touch the wire (TcpNetwork loops them back in-process), so
                    // they don't count as bandwidth — the same rule ProtocolBandwidthTree applies.
                    if link.sender() != link.recipient() {
                        entry_bytes.0 += *size as u64;
                    }
                }
            }
        }

        sim_metrics
    }
}

/// A node of the per-protocol bandwidth call tree reconstructed from one party's trace.
///
/// Each node records the bytes the protocol sent *itself* (excluding its sub-protocols) and one
/// child per sub-protocol invocation, in call order — repeated calls of the same sub-protocol
/// appear as separate children. Build one with [`SimulationOutcome::bandwidth_tree_for`] and
/// export it with [`write_folded`](ProtocolBandwidthTree::write_folded).
pub struct ProtocolBandwidthTree {
    /// The protocol this node stands for. The root stands for the whole run and is named
    /// `<simulation>`.
    id: ProtocolId,
    /// Payload bytes the protocol sent itself, excluding its sub-protocols and self-sends.
    self_bytes: usize,
    /// One node per sub-protocol call, in the order the calls began in the trace.
    children: Vec<ProtocolBandwidthTree>,
}

impl ProtocolBandwidthTree {
    /// Copies the scope `node` of `tree`, and the scopes below it, into a bandwidth tree. `id` names
    /// the root; every other node is named after its protocol.
    fn from_scope(tree: &TraceTree, node: usize, id: ProtocolId) -> Self {
        let scope = &tree.nodes[node];
        Self {
            id: scope.id.unwrap_or(id),
            self_bytes: scope.self_bytes,
            children: scope
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Child(child) => Some(Self::from_scope(tree, *child, id)),
                    Item::Event(_) => None,
                })
                .collect(),
        }
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

/// The root tag of every simulated execution: the tag of the root session that
/// [`simulate`] starts every party in (see [`SessionId::root`](crate::net::session::SessionId::root)).
///
/// A fixed tag is safe here. Every party of a run shares it, and one `simulate` call is one
/// execution: a run cannot exchange messages with another run, so two runs sharing a tag can never
/// confuse each other's sessions. A real deployment must not reuse a tag across executions; see
/// [`TcpNetwork::create`](crate::net::TcpNetwork::create).
pub const SIMULATION_ROOT_TAG: [u8; 32] = *b"scl-rs/simulation/root-tag/v1\0\0\0";

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
///
/// # Panics
///
/// Panics if the parties deadlock: every party that has not finished waits for a packet, and no
/// packet is left to deliver. A receive with a timeout cannot deadlock; it returns
/// [`NetworkError::Timeout`](crate::net::NetworkError::Timeout) instead. The panic message lists
/// each waiting receive, then each packet that reached its recipient but was never received,
/// together with what that recipient is doing. For example, when party 2 waits for party 1 but the
/// packet came from party 0:
///
/// ```text
/// scheduler: 2 task(s) parked, nothing to deliver (deadlock)
/// waiting:
///     party 0 in session root waits for party 2
///     party 2 in session root waits for party 1
/// undelivered:
///     party 2 in session root: 1 packet(s) from party 0; party 2 is waiting in this session, but for a different sender
/// ```
///
/// What the recipient is doing usually points at the cause:
///
/// - *waiting in this session, but for a different sender*: the protocol receives from the wrong
///   party;
/// - *waiting in other sessions*: the parties forked their sessions out of step;
/// - *not waiting*: the recipient has finished, so the packets are leftovers;
/// - *waiting for these packets but never woken*: a bug in the simulator, not in the protocol.
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
