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
    collections::{BinaryHeap, HashMap, VecDeque},
    sync::Arc,
    task::Waker,
    time::Duration,
};

use crate::net::{
    simulation::{
        channel::{Link, NetworkConfig},
        event::{Event, EventType},
        executor::Idle,
        SimulationTrace,
    },
    Packet, PartyId,
};

pub(crate) mod recv;

/// A hook that runs in reaction to events recorded during a simulation.
///
/// Hooks are registered through [`simulate`](crate::net::simulation::simulator::simulate) and fire
/// as each event is appended to a party's trace. They are the extension point for observing or
/// steering a run (for example, injecting a reply when a party receives a particular message).
///
/// `run` is handed `&mut Switchboard`, but only the switchboard's public API is reachable, so a hook cannot corrupt
/// the event queue or recurse back into the recording path.
pub trait TriggeredHook: Send + Sync {
    /// The event type this hook reacts to, or `None` to react to *every* event.
    fn trigger(&self) -> Option<EventType>;
    /// Runs the hook for `party_id` against the just-recorded `event`, with access to the
    /// `switchboard`'s public API.
    fn run(&self, party_id: PartyId, event: &Event, switchboard: &mut Switchboard);
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

/// In-memory message router shared by all party tasks on the scheduler thread.
pub struct Switchboard {
    /// Messages in each link between two parties.
    msg_queues: HashMap<Link, VecDeque<Packet>>,
    /// Waker for a link.
    parked: HashMap<Link, Waker>,
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
    pub(crate) fn send(&mut self, from: PartyId, to: PartyId, packet: Packet) {
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
            link,
            kind: EventKind::Delivery(packet),
        }));
    }

    /// Schedules a wake-up on a link at the virtual instant `deadline`.
    ///
    /// If the awaited packet wins the race, the timer goes stale but stays in the heap; popping it
    /// later is harmless (see `deliver_next`), so no cancellation is needed.
    fn schedule_timer(&mut self, link: Link, deadline: Duration) {
        let seq = self.next_seq();
        self.events.push(Reverse(ScheduledEvent {
            arrival: deadline,
            seq,
            link,
            kind: EventKind::Timer,
        }));
    }

    pub(crate) fn deliver_next(&mut self) -> Idle {
        match self.events.pop() {
            Some(Reverse(event)) => {
                let recipient_clock = self.clocks.entry(event.link.recipient()).or_default();
                // Update the recipient clock for the event. The event may be behind in time.
                //
                // This advance is sound even for a *stale* timer (one whose timed receive already
                // resolved): `deliver_next` only runs once every task is finished or parked, so a
                // party whose clock jumps here is genuinely idle through this instant — anything
                // it still waits for fires later than this event.
                *recipient_clock = (*recipient_clock).max(event.arrival);
                if let EventKind::Delivery(packet) = event.kind {
                    self.msg_queues
                        .entry(event.link)
                        .or_default()
                        .push_back(packet);
                }
                if let Some(waker) = self.parked.remove(&event.link) {
                    waker.wake();
                }
                Idle::Progressed
            }
            None => Idle::Deadlocked,
        }
    }

    /// Returns the current virtual time of `party`, or zero if it has not advanced yet.
    pub fn clock_of(&self, party: PartyId) -> Duration {
        self.clocks.get(&party).copied().unwrap_or_default()
    }

    /// Tries to receive a packet.
    fn try_recv(&mut self, link: Link) -> Option<Packet> {
        let packet = self.msg_queues.get_mut(&link)?.pop_front()?;
        let timestamp = self.clock_of(link.recipient());
        self.record_event(
            link.recipient(),
            Event::ReceiveData {
                timestamp,
                link,
                size: packet.size(),
                content_count: packet.composition(),
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
    fn try_recv_with_deadline(&mut self, link: Link, deadline: Duration) -> TimedRecvOut {
        let timestamp = self.clock_of(link.recipient());
        if let Some(packet) = self
            .msg_queues
            .get_mut(&link)
            .and_then(|queue| queue.pop_front())
        {
            self.record_event(
                link.recipient(),
                Event::ReceiveData {
                    timestamp,
                    link,
                    size: packet.size(),
                    content_count: packet.composition(),
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
    fn park(&mut self, link: Link, waker: Waker) {
        self.parked.insert(link, waker);
    }

    /// Tries to receive the next packet destined for `local` from any of `senders`,
    /// without blocking.
    ///
    /// Among the links that currently have a queued packet, the one with the lowest
    /// sender id is chosen, which keeps the result deterministic and reproducible.
    /// Returns the packet together with the sender it came from, or `None` if no link
    /// has a packet ready.
    fn try_recv_any(&mut self, local: PartyId, senders: &[PartyId]) -> Option<(PartyId, Packet)> {
        let sender = senders
            .iter()
            .copied()
            .filter(|&sender| {
                self.msg_queues
                    .get(&Link::new(sender, local))
                    .is_some_and(|queue| !queue.is_empty())
            })
            .min_by_key(PartyId::as_usize)?;

        // Remove the other peers from the parked list as the future is resolving.
        for &peer in senders {
            self.parked.remove(&Link::new(peer, local));
        }
        let packet = self.try_recv(Link::new(sender, local))?;

        Some((sender, packet))
    }

    fn try_recv_any_with_deadline(
        &mut self,
        local: PartyId,
        senders: &[PartyId],
        deadline: Duration,
    ) -> TimedRecvOut {
        let ready_sender = senders
            .iter()
            .copied()
            .filter(|&sender| {
                self.msg_queues
                    .get(&Link::new(sender, local))
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
            self.parked.remove(&Link::new(peer, local));
        }

        match ready_sender {
            Some(sender) => self.try_recv_with_deadline(Link::new(sender, local), deadline),
            None => TimedRecvOut::Timeout,
        }
    }

    /// Parks `waker` on every link delivering to `local`, so that a send from any of
    /// `senders` wakes the task. Used by [`RecvAny`] to suspend until any peer sends.
    fn park_any(&mut self, local: PartyId, senders: &[PartyId], waker: Waker) {
        for &sender in senders {
            self.parked.insert(Link::new(sender, local), waker.clone());
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
    link: Link,
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
