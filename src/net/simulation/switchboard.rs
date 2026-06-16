use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::Duration,
};

use crate::net::{
    simulation::{
        channel::{ChannelId, NetworkConfig},
        event::{Event, EventType},
        executor::Idle,
        SimulationTrace,
    },
    Packet, PartyId,
};

/// A directed link between two parties, identified from the receiver's side.
#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
pub struct Link {
    /// The recipient (receiver) of messages on this link.
    pub recipient: PartyId,
    /// The sender of messages on this link.
    pub sender: PartyId,
}

impl Link {
    /// The `ChannelId` whose `ChannelConfig` governs this link.
    ///
    /// Network properties are currently symmetric, so the orientation here is
    /// not significant — but we canonicalize so send/recv never disagree even
    /// if a future config becomes direction-sensitive.
    pub fn channel_id(&self) -> ChannelId {
        let (a, b) = (self.sender, self.recipient);
        if a.as_usize() <= b.as_usize() {
            ChannelId::new(a, b)
        } else {
            ChannelId::new(b, a)
        }
    }
}

/// A hook that runs in reaction to events recorded during a simulation.
///
/// Hooks are registered through [`simulate`](crate::net::simulation::runtime::simulate) and fire
/// as each event is appended to a party's trace. They are the extension point for observing or
/// steering a run (for example, injecting a reply when a party receives a particular message).
///
/// `run` is handed `&mut Switchboard`, but only the switchboard's public API is reachable
/// ([`send`](Switchboard::send), [`clock_of`](Switchboard::clock_of), …), so a hook cannot corrupt
/// the event queue or recurse back into the recording path.
pub trait TriggeredHook: Send + Sync {
    /// The event type this hook reacts to, or `None` to react to *every* event.
    fn trigger(&self) -> Option<EventType>;
    /// Runs the hook for `party_id` against the just-recorded `event`, with access to the
    /// `switchboard`'s public API.
    fn run(&self, party_id: PartyId, event: &Event, switchboard: &mut Switchboard);
}

/// In-memory message router shared by all party tasks on the scheduler thread.
pub struct Switchboard {
    /// Messages in each link between two parties.
    msg_queues: HashMap<Link, VecDeque<Packet>>,
    /// Waker for a link.
    parked: HashMap<Link, Waker>,
    /// Enqueued events that are ready to be taken.
    events: BinaryHeap<Reverse<DeliveryEvent>>,
    /// Per party logical times.
    clocks: HashMap<PartyId, Duration>,
    /// The delay model for this switchboard.
    delay: Box<dyn Delay>,
    /// Sequential counter for delivery events.
    seq: usize,
    /// Per-party event traces recorded during the run.
    traces: HashMap<PartyId, SimulationTrace>,
    /// Hooks for the simulation.
    hooks: Vec<Arc<dyn TriggeredHook>>,
}

impl Switchboard {
    /// Creates an empty switchboard that times links with the given `delay` model and fires the
    /// given `hooks` as events are recorded.
    pub fn new(delay: impl Delay + 'static, hooks: Vec<Arc<dyn TriggeredHook>>) -> Self {
        Self {
            traces: HashMap::new(),
            msg_queues: HashMap::new(),
            parked: HashMap::new(),
            events: BinaryHeap::new(),
            clocks: HashMap::new(),
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
    pub fn send(&mut self, from: PartyId, to: PartyId, packet: Packet) {
        let link = Link {
            sender: from,
            recipient: to,
        };

        // Pick the current time of the sender.
        let now = self.clock_of(from);
        self.record_event(
            from,
            Event::SendData {
                timestamp: now,
                channel_id: ChannelId::new(from, to),
                size: packet.size(),
            },
        );

        let arrival_time = now + self.delay.delay(link, packet.size());
        let seq = self.next_seq();
        self.events.push(Reverse(DeliveryEvent {
            arrival: arrival_time,
            seq,
            link,
            packet,
        }));
    }

    pub(crate) fn deliver_next(&mut self) -> Idle {
        match self.events.pop() {
            Some(Reverse(event)) => {
                let recipient_clock = self.clocks.entry(event.link.recipient).or_default();
                // Update the recipient clock for the event. The event may be behind in time.
                *recipient_clock = (*recipient_clock).max(event.arrival);
                self.msg_queues
                    .entry(event.link)
                    .or_default()
                    .push_back(event.packet);
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
        let timestamp = self.clock_of(link.recipient);
        self.record_event(
            link.recipient,
            Event::ReceiveData {
                timestamp,
                channel_id: ChannelId::new(link.recipient, link.sender),
                size: packet.size(),
            },
        );
        Some(packet)
    }

    /// Parks a waker.
    fn park(&mut self, link: Link, waker: Waker) {
        self.parked.insert(link, waker);
    }
}

/// Suspension primitive on receive.
///
/// This primitive waits in a receive instruction, and then resumes when the send is
/// performed. Each link has the possibility to halt until there is some packet available to poll.
pub struct Recv {
    switchboard: Arc<Mutex<Switchboard>>,
    link: Link,
}

impl Recv {
    /// Creates a future that resolves with the next packet `recipient` receives from `sender`,
    /// suspending the task until one is available on that link.
    pub fn new(switchboard: Arc<Mutex<Switchboard>>, sender: PartyId, recipient: PartyId) -> Self {
        Self {
            switchboard,
            link: Link { recipient, sender },
        }
    }
}

impl Future for Recv {
    type Output = Packet;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut switchboard = self.switchboard.lock().expect("the lock must be free");
        match switchboard.try_recv(self.link) {
            Some(packet) => Poll::Ready(packet),
            None => {
                // There is no packet available in the queue, hence you need to wait, i.e. park.
                switchboard.park(self.link, cx.waker().clone());
                Poll::Pending
            }
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
        let channel_id = link.channel_id();
        self.0.channel_config(channel_id).message_delay(size_bytes)
    }
}

/// A delivery event when a packet is sent to a given link.
struct DeliveryEvent {
    /// Arrival time of the event.
    arrival: Duration,
    /// A sequential tie breaker in case two events arrive at the same time.
    seq: usize,
    /// The link associated to the delay.
    link: Link,
    /// The delivered packet.
    packet: Packet,
}

impl Ord for DeliveryEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.arrival
            .cmp(&other.arrival)
            .then(self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for DeliveryEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for DeliveryEvent {
    fn eq(&self, other: &Self) -> bool {
        self.arrival == other.arrival && self.seq == other.seq
    }
}

impl Eq for DeliveryEvent {}
