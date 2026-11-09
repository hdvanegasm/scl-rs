//! In-memory message router and virtual-time scheduler shared by every party in a simulation.
//!
//! A single [`Switchboard`](crate::net::simulation::switchboard::Switchboard) backs all parties on
//! the simulator's single-threaded core. It owns the per-link message queues, the parked wakers of
//! suspended receives, and a priority queue of scheduled events ordered by virtual time. `send`
//! enqueues a delivery at the sender's clock plus a link delay (see
//! [`Delay`](crate::net::simulation::switchboard::Delay)), and the executor calls `deliver_next`
//! whenever no party can make progress, advancing virtual time to the next event and waking the
//! receiver.
//!
//! The receive-side futures that suspend a party until a packet (or a timeout) arrives live in the
//! `recv` submodule; they drive the switchboard through its internal try-receive/park API.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, VecDeque},
    fmt::{self, Display, Formatter},
    sync::Arc,
    task::Waker,
    time::Duration,
};

use crate::net::{
    session::SessionId,
    simulation::{
        channel::{Link, NetworkConfig},
        event::Event,
        executor::IdleOutcome,
        hook::TriggeredHook,
        SimulationTrace,
    },
    Packet, PartyId,
};

pub(crate) mod recv;

/// What the switchboard holds when the simulation deadlocks: the receives still waiting, and the
/// packets delivered to a mailbox that no receive took. Built by [`Switchboard::deadlock_report`]
/// and printed in the executor's deadlock panic. Both maps are sorted, so the report reads the
/// same in every run.
///
/// A waiting receive is an entry in `parked`, and nothing removes the entry of a receive future
/// that is dropped while it waits. Such a receive is still listed as waiting. This happens when
/// protocol code drops a pending receive, e.g. a `select!` over two receives, or a combinator that
/// cancels its other branches when one of them fails.
pub(crate) struct DeadlockReport {
    /// `(recipient, session)` → the senders that party waits for in that session. A `recv_any`
    /// waits for every party, the recipient included.
    waiting: BTreeMap<(PartyId, SessionId), BTreeSet<PartyId>>,
    /// `(recipient, session, sender)` → the number of packets left in that mailbox.
    undelivered: BTreeMap<(PartyId, SessionId, PartyId), usize>,
}

/// Prints one line per waiting receive, then one line per mailbox with undelivered packets. Each
/// mailbox line says what its recipient is doing when the run deadlocks:
///
/// - *waiting for these packets but never woken*: a lost wake-up, which is a bug in the
///   simulator, not in the protocol;
/// - *waiting in this session, but for a different sender*: usually a protocol logic bug;
/// - *waiting in other sessions*: usually a symmetric-forking violation, the parties' fork
///   counters out of step;
/// - *not waiting*: the recipient has finished, and the packets are leftovers.
impl Display for DeadlockReport {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        writeln!(f, "waiting:")?;
        if self.waiting.is_empty() {
            writeln!(f, "\t(none)")?;
        }
        for ((waiter, sid), senders) in &self.waiting {
            let senders: Vec<String> = senders.iter().map(ToString::to_string).collect();
            writeln!(
                f,
                "\tparty {waiter} in session {sid} waits for party {}",
                senders.join(" or ")
            )?;
        }

        writeln!(f, "undelivered:")?;
        if self.undelivered.is_empty() {
            writeln!(f, "\t(none)")?;
        }
        for ((recipient, sid, sender), n_msg) in &self.undelivered {
            let note = match self.waiting.get(&(*recipient, sid.clone())) {
                Some(senders) if senders.contains(sender) => {
                    "is waiting for these packets but was never woken: a lost wake-up, which is a bug in the simulator, not in the protocol"
                },
                Some(_) => "is waiting in this session, but for a different sender",
                None if self.waiting.keys().any(|(waiter, _)| waiter == recipient) => {
                    "is waiting in other sessions"
                },
                None => "is not waiting",
            };

            writeln!(
                f,
                "\tparty {recipient} in session {sid}: {n_msg} packet(s) from party {sender}; \
                 party {recipient} {note}"
            )?;
        }
        Ok(())
    }
}

/// Outcome of one try-receive attempt under a deadline. Shared by the single-link and any-link
/// receives: a successful receive always carries the sender (the single-link caller ignores it),
/// so both use the same shape.
enum TimedRecvOut {
    /// A packet was ready, together with the party that sent it.
    Some((PartyId, Packet)),
    /// No packet was ready and the recipient's clock has reached the deadline.
    Timeout,
    /// No packet yet, deadline not reached; the caller should park.
    None,
}

/// Names one mailbox of the switchboard: the packets that travel on one directed link within one
/// session.
///
/// Message queues, parked wakers and scheduled events are keyed by it, so sessions that share a
/// link never take each other's packets or wake each other's receives. Per-link FIFO delivery is
/// deliberately keyed by the [`Link`] alone: all sessions on a link share one stream, so a packet
/// never overtakes one sent earlier on the same link, whatever its session.
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct MailboxKey {
    link: Link,
    session_id: SessionId,
}

impl MailboxKey {
    /// Returns the key of the mailbox for packets on `link` within session `sid`.
    pub fn new(link: Link, sid: SessionId) -> Self {
        Self {
            link,
            session_id: sid,
        }
    }

    /// Returns the directed link (`sender` → `recipient`) of this mailbox.
    pub fn link(&self) -> Link {
        self.link
    }

    /// Returns the session this mailbox belongs to.
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }
}

/// In-memory message router shared by all party tasks on the scheduler thread.
pub struct Switchboard {
    /// Messages in each link between two parties.
    msg_queues: HashMap<MailboxKey, VecDeque<Packet>>,
    /// Waker for a link.
    parked: HashMap<MailboxKey, Waker>,
    /// Enqueued events that are ready to be taken.
    events: BinaryHeap<Reverse<ScheduledEvent>>,
    /// Per party logical times.
    clocks: HashMap<PartyId, Duration>,
    /// The arrival time of the last message scheduled on each link, used to keep per-link
    /// delivery FIFO (see [`send`](Switchboard::send)).
    last_arrivals: HashMap<Link, Duration>,
    /// The delay model for this switchboard.
    delay: Box<dyn Delay>,
    /// Sequential counter for scheduled events.
    seq: usize,
    /// Per-party event traces recorded during the run.
    traces: HashMap<PartyId, SimulationTrace>,
    /// Hooks for the simulation.
    hooks: Vec<Arc<dyn TriggeredHook>>,
}

impl Switchboard {
    /// Creates an empty switchboard that times links with the given `delay` model and fires the
    /// given `hooks` as events are recorded.
    pub(crate) fn new(delay: impl Delay + 'static, hooks: Vec<Arc<dyn TriggeredHook>>) -> Self {
        Self {
            traces: HashMap::new(),
            msg_queues: HashMap::new(),
            parked: HashMap::new(),
            events: BinaryHeap::new(),
            clocks: HashMap::new(),
            last_arrivals: HashMap::new(),
            delay: Box::new(delay),
            hooks,
            seq: 0,
        }
    }

    pub(crate) fn take_traces(&mut self) -> HashMap<PartyId, SimulationTrace> {
        std::mem::take(&mut self.traces)
    }

    fn next_seq(&mut self) -> usize {
        let returned_seq = self.seq;
        self.seq += 1;
        returned_seq
    }

    pub(crate) fn record_event(&mut self, party: PartyId, event: Event) {
        self.traces
            .entry(party)
            .or_insert_with(SimulationTrace::empty)
            .add_event(event.clone());

        // Collect matching hooks (clone the Arcs) so we can hand `&mut self` to `run`.
        let triggered: Vec<Arc<dyn TriggeredHook>> = self
            .hooks
            .iter()
            .filter(|hook| hook.trigger().is_none_or(|t| t == event.event_type()))
            .cloned()
            .collect();
        for hook in triggered {
            hook.run(party, &event, self);
        }
    }

    /// Send a message to another party.
    ///
    /// Schedules a delivery event at the sender's current virtual time plus the link delay; the
    /// event loop (`deliver_next`) delivers it and wakes the recipient later.
    ///
    /// Deliveries on the same link are kept **FIFO**: a message never arrives before one sent
    /// earlier on the same link, even when the size-dependent delay model would give the later
    /// (smaller) message a shorter transit time. This mirrors the real backend — a `TcpNetwork`
    /// link is a single TCP stream, where bytes cannot overtake each other — so protocols observe
    /// the same per-link ordering on both backends.
    pub(crate) fn send(&mut self, session: SessionId, from: PartyId, to: PartyId, packet: Packet) {
        let link = Link::new(from, to);

        // Pick the current time of the sender.
        let now = self.clock_of(from);
        self.record_event(
            from,
            Event::SendData {
                timestamp: now,
                link,
                size: packet.size(),
                content_count: packet.composition(),
                session: session.clone(),
            },
        );

        // Clamp the arrival so it is not earlier than the previously scheduled arrival on this
        // link (per-link FIFO); ties are broken by `seq`, which preserves the send order.
        let mut arrival_time = now + self.delay.delay(link, packet.size());
        if let Some(&last_arrival) = self.last_arrivals.get(&link) {
            arrival_time = arrival_time.max(last_arrival);
        }
        self.last_arrivals.insert(link, arrival_time);
        let seq = self.next_seq();
        self.events.push(Reverse(ScheduledEvent {
            arrival: arrival_time,
            seq,
            key: MailboxKey::new(link, session),
            kind: EventKind::Delivery(packet),
        }));
    }

    /// Schedules a wake-up on a link at the virtual instant `deadline`.
    ///
    /// If the awaited packet wins the race, the timer goes stale but stays in the heap; popping it
    /// later is harmless (see `deliver_next`), so no cancellation is needed.
    fn schedule_timer(&mut self, session: SessionId, link: Link, deadline: Duration) {
        let seq = self.next_seq();
        self.events.push(Reverse(ScheduledEvent {
            arrival: deadline,
            seq,
            key: MailboxKey::new(link, session),
            kind: EventKind::Timer,
        }));
    }

    /// Snapshots what is left when the event heap is empty: each parked receive, grouped by
    /// recipient and session, and each mailbox that still holds packets, with their number.
    /// Mailboxes that receives have emptied stay in `msg_queues` and are skipped.
    pub(crate) fn deadlock_report(&self) -> DeadlockReport {
        let mut waiting: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
        for key in self.parked.keys() {
            waiting
                .entry((key.link().recipient(), key.session_id().clone()))
                .or_default()
                .insert(key.link.sender());
        }

        let undelivered = self
            .msg_queues
            .iter()
            .filter(|(_, queue)| !queue.is_empty())
            .map(|(key, queue)| {
                (
                    (
                        key.link().recipient(),
                        key.session_id.clone(),
                        key.link().sender(),
                    ),
                    queue.len(),
                )
            })
            .collect();
        DeadlockReport {
            waiting,
            undelivered,
        }
    }

    /// Takes the next scheduled event and executes it.
    ///
    /// If there is an event in the `switchboard` that makes the simulation to progress, then the
    /// function returns that the simulation progressed, otherwise, it returns that the simulation is
    /// deadlocked. The deadlock occurs when all tasks are parked and there is no event that makes
    /// the simulation to progress. The deadlocked outcome carries a [`DeadlockReport`] of the
    /// receives still waiting and the packets that no receive took.
    pub(crate) fn deliver_next(&mut self) -> IdleOutcome<DeadlockReport> {
        match self.events.pop() {
            Some(Reverse(event)) => {
                // Update the recipient clock for the event. The event may be behind in time.
                //
                // This advance is sound even for a *stale* timer (one whose timed receive already
                // resolved): `deliver_next` only runs once every task is finished or parked, so a
                // party whose clock jumps here is genuinely idle through this instant — anything
                // it still waits for fires later than this event.
                let recipient_clock = self.clocks.entry(event.key.link().recipient()).or_default();
                *recipient_clock = (*recipient_clock).max(event.arrival);
                if let EventKind::Delivery(packet) = event.kind {
                    self.msg_queues
                        .entry(event.key.clone())
                        .or_default()
                        .push_back(packet);
                }
                // We can wake the current link as the task may progress with the new delivered message.
                if let Some(waker) = self.parked.remove(&event.key) {
                    waker.wake();
                }
                IdleOutcome::Progressed
            }
            // There are no more events in the queue, and this function is called on idle. Hence the
            // simulator has no way to continue.
            None => IdleOutcome::Deadlocked(self.deadlock_report()),
        }
    }

    /// Returns the current virtual time of `party`, or zero if it has not advanced yet.
    pub fn clock_of(&self, party: PartyId) -> Duration {
        self.clocks.get(&party).copied().unwrap_or_default()
    }

    /// Tries to receive a packet.
    fn try_recv(&mut self, session: SessionId, link: Link) -> Option<Packet> {
        let mailbox = MailboxKey::new(link, session.clone());
        let packet = self.msg_queues.get_mut(&mailbox)?.pop_front()?;
        let timestamp = self.clock_of(link.recipient());
        self.record_event(
            link.recipient(),
            Event::ReceiveData {
                timestamp,
                link,
                size: packet.size(),
                content_count: packet.composition(),
                session,
            },
        );
        Some(packet)
    }

    /// Tries to receive a packet, giving up once the recipient's clock reaches `deadline`.
    ///
    /// The queue is checked *before* the deadline so that a packet delivered exactly at the
    /// deadline instant wins the tie, matching `tokio::time::timeout` on the real backend (which
    /// polls the inner future before checking the deadline). A packet arriving strictly after the
    /// deadline can never be returned here: its delivery event pops after the timer event, so the
    /// timed receive has already resolved to `Timeout` by then (the late packet stays queued for a
    /// later receive, as the bytes would on a real TCP stream).
    fn try_recv_with_deadline(
        &mut self,
        session: SessionId,
        link: Link,
        deadline: Duration,
    ) -> TimedRecvOut {
        let timestamp = self.clock_of(link.recipient());
        let mailbox = MailboxKey::new(link, session.clone());
        if let Some(packet) = self
            .msg_queues
            .get_mut(&mailbox)
            .and_then(|queue| queue.pop_front())
        {
            self.record_event(
                link.recipient(),
                Event::ReceiveData {
                    timestamp,
                    link,
                    size: packet.size(),
                    content_count: packet.composition(),
                    session,
                },
            );
            return TimedRecvOut::Some((link.sender(), packet));
        }
        // The timer event sets the recipient's clock to *exactly* `deadline`, so this check must
        // be inclusive; a strict comparison would park the task again with no event left to wake
        // it.
        if timestamp >= deadline {
            TimedRecvOut::Timeout
        } else {
            TimedRecvOut::None
        }
    }

    /// Parks a waker.
    fn park(&mut self, session: SessionId, link: Link, waker: Waker) {
        let mb = MailboxKey::new(link, session);
        self.parked.insert(mb, waker);
    }

    /// Tries to receive the next packet destined for `local` from any of `senders`,
    /// without blocking.
    ///
    /// Among the links that currently have a queued packet, the one with the lowest
    /// sender id is chosen, which keeps the result deterministic and reproducible.
    /// Returns the packet together with the sender it came from, or `None` if no link
    /// has a packet ready.
    fn try_recv_any(
        &mut self,
        session: &SessionId,
        local: PartyId,
        senders: &[PartyId],
    ) -> Option<(PartyId, Packet)> {
        let sender = senders
            .iter()
            .copied()
            .filter(|&sender| {
                let link = Link::new(sender, local);
                let mb = MailboxKey::new(link, session.clone());
                self.msg_queues
                    .get(&mb)
                    .is_some_and(|queue| !queue.is_empty())
            })
            .min_by_key(PartyId::as_usize)?;

        // Remove the other peers from the parked list as the future is resolving.
        for &peer in senders {
            let link = Link::new(peer, local);
            let mb = MailboxKey::new(link, session.clone());
            self.parked.remove(&mb);
        }
        let link = Link::new(sender, local);
        let packet = self.try_recv(session.clone(), link)?;

        Some((sender, packet))
    }

    fn try_recv_any_with_deadline(
        &mut self,
        session: &SessionId,
        local: PartyId,
        senders: &[PartyId],
        deadline: Duration,
    ) -> TimedRecvOut {
        let ready_sender = senders
            .iter()
            .copied()
            .filter(|&sender| {
                let link = Link::new(sender, local);
                let mailbox = MailboxKey::new(link, session.clone());
                self.msg_queues
                    .get(&mailbox)
                    .is_some_and(|queue| !queue.is_empty())
            })
            .min_by_key(PartyId::as_usize);

        // Nothing is ready and the deadline has not passed: keep waiting. Return `None` so the
        // caller re-parks, leaving the wakers on every inbound link in place.
        if ready_sender.is_none() && self.clock_of(local) < deadline {
            return TimedRecvOut::None;
        }

        // The receive is resolving now — with a packet or a timeout — and will not park again, so
        // drop the wakers parked on every inbound link before returning (as the single-sender
        // success path in `try_recv_any` does).
        for &peer in senders {
            let link = Link::new(peer, local);
            let mailbox = MailboxKey::new(link, session.clone());
            self.parked.remove(&mailbox);
        }

        match ready_sender {
            Some(sender) => {
                self.try_recv_with_deadline(session.clone(), Link::new(sender, local), deadline)
            }
            None => TimedRecvOut::Timeout,
        }
    }

    /// Parks `waker` on every link delivering to `local`, so that a send from any of
    /// `senders` wakes the task. Used by [`RecvAny`] to suspend until any peer sends.
    fn park_any(&mut self, session: &SessionId, local: PartyId, senders: &[PartyId], waker: Waker) {
        for &sender in senders {
            let link = Link::new(sender, local);
            let mailbox = MailboxKey::new(link, session.clone());
            self.parked.insert(mailbox, waker.clone());
        }
    }
}

/// Computes a network delay over a link.
pub trait Delay: Send {
    /// The delay for the link.
    fn delay(&self, link: Link, size_bytes: usize) -> Duration;
}

/// A constant delay for all the links in the network.
#[derive(Debug)]
pub struct ConstantDelay(pub Duration);

impl Delay for ConstantDelay {
    fn delay(&self, _link: Link, _size_bytes: usize) -> Duration {
        self.0
    }
}

/// A delay following a network configuration.
#[derive(Debug)]
pub struct ConfigDelay<N>(pub N)
where
    N: NetworkConfig;

impl<N> Delay for ConfigDelay<N>
where
    N: NetworkConfig,
{
    fn delay(&self, link: Link, size_bytes: usize) -> Duration {
        self.0.channel_config(link).message_delay(size_bytes)
    }
}

/// What a [`ScheduledEvent`] does when it fires.
enum EventKind {
    /// Delivers a packet on the link.
    Delivery(Packet),
    /// Wakes a timed receive parked on the link so it can observe that its deadline passed.
    Timer,
}

/// An event scheduled on the switchboard's virtual timeline: a packet delivery or a
/// recv-timeout deadline on a link.
struct ScheduledEvent {
    /// Virtual instant at which the event fires (a packet's arrival, or a timer's deadline).
    arrival: Duration,
    /// A sequential tie breaker in case two events fire at the same time.
    seq: usize,
    /// The link the event fires on.
    key: MailboxKey,
    /// What happens when the event fires.
    kind: EventKind,
}

impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.arrival
            .cmp(&other.arrival)
            .then(self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ScheduledEvent {
    fn eq(&self, other: &Self) -> bool {
        self.arrival == other.arrival && self.seq == other.seq
    }
}

impl Eq for ScheduledEvent {}

/// Phase 1 of the concurrent-composition plan (`docs/concurrent-composition-plan.md`) at the
/// switchboard level: session routing (tests T3, T4 and T5 of §8) and the deadlock report (§5.2).
/// They drive `send`, `deliver_next`, `try_recv*` and `park*` directly with explicit sessions, so
/// they need no fork API.
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Wake;

    use super::*;
    use crate::net::simulation::simulator::SIMULATION_ROOT_TAG;

    /// A switchboard with a 1 ms delay on every link and no hooks.
    fn switchboard() -> Switchboard {
        Switchboard::new(ConstantDelay(Duration::from_millis(1)), Vec::new())
    }

    fn party(id: usize) -> PartyId {
        PartyId::from(id)
    }

    /// The root session of a simulated execution.
    fn root() -> SessionId {
        SessionId::root(SIMULATION_ROOT_TAG)
    }

    /// A packet holding `tag`, so that packets can be told apart by equality.
    fn packet(tag: u64) -> Packet {
        let mut packet = Packet::empty();
        packet.write(&tag).unwrap();
        packet
    }

    /// Counts how many times the waker built from it has been woken.
    struct WakeCounter(AtomicUsize);

    impl WakeCounter {
        fn count(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }

    impl Wake for WakeCounter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Returns a waker together with the counter its wakes increment.
    fn counting_waker() -> (Waker, Arc<WakeCounter>) {
        let counter = Arc::new(WakeCounter(AtomicUsize::new(0)));
        (Waker::from(Arc::clone(&counter)), counter)
    }

    /// T3: isolation under adversarial ordering. On one link, session B's packet is sent and
    /// delivered before session A's, while the receiver is already waiting in A. B's delivery
    /// neither wakes A's waiter nor reaches A's mailbox, and each session receives exactly its own
    /// packet. On a switchboard keyed by link alone, A's receive would take B's packet.
    #[test]
    fn sessions_on_one_link_receive_only_their_own_packets() {
        let mut sb = switchboard();
        let (p0, p1) = (party(0), party(1));
        let link = Link::new(p0, p1);
        let session_a = root().with_child(0);
        let session_b = root().with_child(1);

        // The receiver waits in A first.
        let (waker_a, woken_a) = counting_waker();
        assert_eq!(sb.try_recv(session_a.clone(), link), None);
        sb.park(session_a.clone(), link, waker_a);

        // B sends before A on the same link, so B's packet is delivered first.
        sb.send(session_b.clone(), p0, p1, packet(2));
        sb.send(session_a.clone(), p0, p1, packet(1));

        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));
        assert_eq!(woken_a.count(), 0, "B's delivery must not wake A's waiter");
        assert_eq!(sb.try_recv(session_a.clone(), link), None);

        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));
        assert_eq!(woken_a.count(), 1, "A's delivery must wake A's waiter");
        assert_eq!(sb.try_recv(session_a.clone(), link), Some(packet(1)));
        assert_eq!(sb.try_recv(session_b.clone(), link), Some(packet(2)));

        assert_eq!(sb.try_recv(session_a, link), None);
        assert_eq!(sb.try_recv(session_b, link), None);
    }

    /// Which waiter parks first in [`lost_wake_up_scenario`].
    #[derive(Clone, Copy, Debug)]
    enum ParkOrder {
        AnyFirst,
        SingleFirst,
    }
    /// Which try-receive resolves the receive-any in [`lost_wake_up_scenario`]. Each one has its own loop that removes the receive-any's wakers.
    #[derive(Clone, Copy, Debug)]
    enum AnyRecv {
        Untimed,
        Timed,
    }

    /// T4: party 2 waits in two sessions at once, both on the link 0 → 2: a receive-any from
    /// parties 0 and 1 in session A, and a receive from party 0 in session B. A's packet arrives
    /// and the receive-any resolves. B's packet then arrives on 0 → 2, and B's waiter must still
    /// be parked to be woken.
    fn lost_wake_up_scenario(order: ParkOrder, any_recv: AnyRecv) {
        let case = format!("{order:?}, {any_recv:?}");
        let mut sb = switchboard();
        let (p0, p1, p2) = (party(0), party(1), party(2));
        let senders = [p0, p1];
        let shared = Link::new(p0, p2);
        let session_a = root().with_child(0);
        let session_b = root().with_child(1);
        let (waker_any, woken_any) = counting_waker();
        let (waker_single, woken_single) = counting_waker();

        match order {
            ParkOrder::AnyFirst => {
                sb.park_any(&session_a, p2, &senders, waker_any);
                sb.park(session_b.clone(), shared, waker_single);
            }
            ParkOrder::SingleFirst => {
                sb.park(session_b.clone(), shared, waker_single);
                sb.park_any(&session_a, p2, &senders, waker_any);
            }
        }

        // A's packet comes from party 1, off the shared link, so the only thing that can remove
        // B's waiter is the receive-any resolving.
        sb.send(session_a.clone(), p1, p2, packet(1));
        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));
        assert_eq!(
            woken_any.count(),
            1,
            "{case}: A's delivery must wake A's waiter"
        );
        assert_eq!(
            woken_single.count(),
            0,
            "{case}: A's delivery must not wake B's waiter"
        );

        let received = match any_recv {
            AnyRecv::Untimed => sb.try_recv_any(&session_a, p2, &senders),
            AnyRecv::Timed => {
                let deadline = Duration::from_secs(1);
                match sb.try_recv_any_with_deadline(&session_a, p2, &senders, deadline) {
                    TimedRecvOut::Some(received) => Some(received),
                    TimedRecvOut::Timeout | TimedRecvOut::None => None,
                }
            }
        };
        assert_eq!(received, Some((p1, packet(1))), "{case}");

        sb.send(session_b.clone(), p0, p2, packet(2));
        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));
        assert_eq!(woken_single.count(), 1, "{case}: B's wake-up was lost");
        assert_eq!(sb.try_recv(session_b, shared), Some(packet(2)), "{case}");
    }

    #[test]
    fn resolving_a_receive_any_keeps_other_sessions_waiters() {
        for order in [ParkOrder::AnyFirst, ParkOrder::SingleFirst] {
            lost_wake_up_scenario(order, AnyRecv::Untimed);
        }
    }

    #[test]
    fn resolving_a_timed_receive_any_keeps_other_sessions_waiters() {
        for order in [ParkOrder::AnyFirst, ParkOrder::SingleFirst] {
            lost_wake_up_scenario(order, AnyRecv::Timed);
        }
    }

    /// T5: early arrival. A packet sent in session 0.3 before the receiver has ever used that
    /// session is buffered, and is received once the receiver asks for it. Neither the root
    /// session nor the parent session 0 sees it.
    #[test]
    fn packet_for_an_unopened_session_is_buffered_until_received() {
        let mut sb = switchboard();
        let (p0, p1) = (party(0), party(1));
        let link = Link::new(p0, p1);
        let early = root().with_child(0).with_child(3);

        sb.send(early.clone(), p0, p1, packet(7));
        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));

        assert_eq!(sb.try_recv(root(), link), None);
        assert_eq!(sb.try_recv(root().with_child(0), link), None);

        assert_eq!(sb.try_recv(early.clone(), link), Some(packet(7)));
        assert_eq!(sb.try_recv(early, link), None);
    }

    /// The notes the deadlock report's `Display` gives each mailbox with undelivered packets,
    /// printed after `party <recipient> `: what the recipient is doing when the run deadlocks.
    /// The `Display` impl must print these exact phrases.
    const SAME_SESSION: &str = "is waiting in this session, but for a different sender";
    const OTHER_SESSIONS: &str = "is waiting in other sessions";
    const NOT_WAITING: &str = "is not waiting";
    const NOTES: [&str; 3] = [SAME_SESSION, OTHER_SESSIONS, NOT_WAITING];

    /// Calls `deliver_next` on a switchboard with no scheduled events and returns its report.
    fn deadlock_report_of(sb: &mut Switchboard) -> DeadlockReport {
        match sb.deliver_next() {
            IdleOutcome::Deadlocked(report) => report,
            IdleOutcome::Progressed => panic!("an empty event heap must report a deadlock"),
        }
    }

    /// Asserts that the report `text` gives `recipient` the note `expected` and no other note.
    fn assert_note(text: &str, recipient: PartyId, expected: &str) {
        for note in NOTES {
            let line = format!("party {} {note}", recipient.as_usize());
            if note == expected {
                assert!(
                    text.contains(&line),
                    "missing {line:?} in the report:\n{text}"
                );
            } else {
                assert!(
                    !text.contains(&line),
                    "unexpected {line:?} in the report:\n{text}"
                );
            }
        }
    }

    /// Deadlock report, recipient waiting in the same session for another sender: two packets
    /// from party 0 reach party 2 in the root session, but party 2 waits there for party 1. In a
    /// protocol, this is a logic bug: it expects the message from the wrong party.
    #[test]
    fn deadlock_report_flags_a_recipient_waiting_for_another_sender() {
        let mut sb = switchboard();
        let (p0, p1, p2) = (party(0), party(1), party(2));
        let (waker, _) = counting_waker();
        sb.park(root(), Link::new(p1, p2), waker);
        sb.send(root(), p0, p2, packet(1));
        sb.send(root(), p0, p2, packet(2));
        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));
        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));

        let report = deadlock_report_of(&mut sb);
        assert_eq!(report.undelivered, BTreeMap::from([((p2, root(), p0), 2)]));
        assert_eq!(
            report.waiting,
            BTreeMap::from([((p2, root()), BTreeSet::from([p1]))])
        );
        assert_note(&report.to_string(), p2, SAME_SESSION);
    }

    /// Deadlock report, recipient waiting only in other sessions: party 0's packet reaches party 1
    /// in session 4, while party 1 waits for party 0 in session 3. This is the symptom of the
    /// parties' fork counters going out of step (invariant I1); test T10 checks the same through
    /// `simulate` in Phase 2.
    #[test]
    fn deadlock_report_names_the_orphaned_session_and_where_the_recipient_waits() {
        let mut sb = switchboard();
        let (p0, p1) = (party(0), party(1));
        let orphan = root().with_child(4);
        let open = root().with_child(3);
        sb.send(orphan.clone(), p0, p1, packet(1));
        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));
        let (waker, _) = counting_waker();
        sb.park(open.clone(), Link::new(p0, p1), waker);

        let report = deadlock_report_of(&mut sb);
        assert_eq!(
            report.undelivered,
            BTreeMap::from([((p1, orphan.clone(), p0), 1)])
        );
        assert_eq!(
            report.waiting,
            BTreeMap::from([((p1, open.clone()), BTreeSet::from([p0]))])
        );
        let text = report.to_string();
        for session in [&orphan, &open] {
            let name = format!("session {session}");
            assert!(
                text.contains(&name),
                "missing {name:?} in the report:\n{text}"
            );
        }
        assert_note(&text, p1, OTHER_SESSIONS);
    }

    /// Deadlock report, recipient not waiting: party 1 never consumes party 0's two packets, while
    /// party 0 waits for a reply from party 1. In Phase 1 every unfinished party has a parked
    /// receive, so party 1 has finished.
    #[test]
    fn deadlock_report_flags_packets_whose_recipient_is_not_waiting() {
        let mut sb = switchboard();
        let (p0, p1) = (party(0), party(1));
        sb.send(root(), p0, p1, packet(1));
        sb.send(root(), p0, p1, packet(2));
        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));
        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));
        let (waker, _) = counting_waker();
        sb.park(root(), Link::new(p1, p0), waker);

        let report = deadlock_report_of(&mut sb);
        assert_eq!(report.undelivered, BTreeMap::from([((p1, root(), p0), 2)]));
        assert_eq!(
            report.waiting,
            BTreeMap::from([((p0, root()), BTreeSet::from([p1]))])
        );
        assert_note(&report.to_string(), p1, NOT_WAITING);
    }

    /// Deadlock report, pure wait cycle: party 0 waits for party 1, and party 1 waits for any
    /// party, itself included, as `SimNetwork::recv_any` does. No packet is left over: the one
    /// exchanged earlier was consumed, which leaves an empty queue in `msg_queues`. The report
    /// must still list both waiting receives, and no undelivered packets.
    #[test]
    fn deadlock_report_lists_the_waiting_receives_of_a_wait_cycle() {
        let mut sb = switchboard();
        let (p0, p1) = (party(0), party(1));
        sb.send(root(), p0, p1, packet(1));
        assert!(matches!(sb.deliver_next(), IdleOutcome::Progressed));
        assert_eq!(sb.try_recv(root(), Link::new(p0, p1)), Some(packet(1)));

        let (waker_0, _) = counting_waker();
        let (waker_1, _) = counting_waker();
        sb.park(root(), Link::new(p1, p0), waker_0);
        sb.park_any(&root(), p1, &[p0, p1], waker_1);

        let report = deadlock_report_of(&mut sb);
        assert_eq!(report.undelivered, BTreeMap::new());
        assert_eq!(
            report.waiting,
            BTreeMap::from([
                ((p0, root()), BTreeSet::from([p1])),
                ((p1, root()), BTreeSet::from([p0, p1])),
            ])
        );
        let text = report.to_string();
        for note in NOTES {
            assert!(
                !text.contains(note),
                "nothing is undelivered, yet the report says {note:?}:\n{text}"
            );
        }
    }
}
