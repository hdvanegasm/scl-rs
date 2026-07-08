//! Receive-side suspension futures that back [`SimNetwork`](crate::net::simulation::network::SimNetwork).
//!
//! Each type adapts the synchronous try-receive/park API of the parent
//! [`Switchboard`](super::Switchboard) into a [`Future`] the simulator's executor can poll:
//!
//! - [`Recv`] resolves with the next packet on a single link.
//! - [`RecvAny`] resolves with the next packet arriving from *any* peer, together with its sender.
//! - [`RecvTimeout`] resolves with a packet, or [`NetworkError::Timeout`] once the recipient's
//!   virtual clock reaches the deadline.
//! - [`RecvAnyTimeout`] combines the last two: the next packet from any peer, or
//!   [`NetworkError::Timeout`] once the deadline passes.
//!
//! Every future holds an `Arc<Mutex<Switchboard>>`. On each poll it locks the switchboard and tries
//! to take a packet; if none is ready it parks its waker on the relevant link, so a later delivery
//! (or a timer firing) re-polls it.

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use crate::net::{simulation::channel::Link, NetworkError, Packet, PartyId};

use super::{Switchboard, TimedRecvOut};

/// Suspension primitive that suspends until any party sends a message.
///
/// It is similar to [`Recv`], where the difference is that instead of waiting on a link, it waits
/// on all the links delivering messages to `local`. This future resolves imediately after a link
/// gets a message.
pub(crate) struct RecvAny {
    switchboard: Arc<Mutex<Switchboard>>,
    local: PartyId,
    senders: Vec<PartyId>,
}

impl RecvAny {
    /// Creates a new future resolving to a `(packet, sender)` that local receives from any party in
    /// `senders`.
    pub(crate) fn new(
        switchboard: Arc<Mutex<Switchboard>>,
        local: PartyId,
        senders: Vec<PartyId>,
    ) -> Self {
        Self {
            switchboard,
            local,
            senders,
        }
    }
}

impl Future for RecvAny {
    type Output = (PartyId, Packet);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut switchboard = self.switchboard.lock().expect("the lock must be free");
        match switchboard.try_recv_any(self.local, &self.senders) {
            Some(result) => Poll::Ready(result),
            None => {
                switchboard.park_any(self.local, &self.senders, cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Suspension primitive on receive.
///
/// This primitive waits in a receive instruction, and then resumes when the send is
/// performed. Each link has the possibility to halt until there is some packet available to poll.
pub(crate) struct Recv {
    switchboard: Arc<Mutex<Switchboard>>,
    link: Link,
}

impl Recv {
    /// Creates a future that resolves with the next packet `recipient` receives from `sender`,
    /// suspending the task until one is available on that link.
    pub(crate) fn new(
        switchboard: Arc<Mutex<Switchboard>>,
        sender: PartyId,
        recipient: PartyId,
    ) -> Self {
        Self {
            switchboard,
            link: Link::new(sender, recipient),
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

/// Suspension primitive on receive, bounded by a timeout.
///
/// Like [`Recv`] it waits for the next packet on a single link, but gives up once the recipient's
/// virtual clock passes a deadline. The first poll schedules a timer on the switchboard at that
/// deadline, so the task is still woken to observe the timeout even if no packet ever arrives; the
/// future then resolves with the packet if one is delivered in time, or with
/// [`NetworkError::Timeout`] once the deadline passes.
pub(crate) struct RecvTimeout {
    /// The shared router the packet is awaited on.
    switchboard: Arc<Mutex<Switchboard>>,
    /// The directed link (`sender` -> `recipient`) the packet is expected on.
    link: Link,
    /// How long to wait for the packet, measured from the first poll, before timing out.
    timeout: Duration,
    /// Deadline for the timeout.
    ///
    /// Any time past this deadline is considered a timeout. This deadline is computed as the
    /// virtual instant at which the receive is first polled plus the timeout, and stays `None`
    /// until that first poll sets it.
    deadline: Option<Duration>,
}

impl RecvTimeout {
    /// Creates a future that resolves with the next packet `recipient` receives from `sender`,
    /// suspending the task until one is available on that link.
    pub(crate) fn new(
        switchboard: Arc<Mutex<Switchboard>>,
        sender: PartyId,
        recipient: PartyId,
        timeout: Duration,
    ) -> Self {
        Self {
            switchboard,
            link: Link::new(sender, recipient),
            timeout,
            deadline: None,
        }
    }
}

impl Future for RecvTimeout {
    type Output = Result<Packet, NetworkError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut switchboard = this.switchboard.lock().expect("the lock must be free");
        let deadline = match this.deadline {
            Some(d) => d,
            // Here we are dealing with the first call to recv.
            None => {
                let deadline = switchboard.clock_of(this.link.recipient()) + this.timeout;
                switchboard.schedule_timer(this.link, deadline);
                this.deadline = Some(deadline);
                deadline
            }
        };
        match switchboard.try_recv_with_deadline(this.link, deadline) {
            TimedRecvOut::Some((_, packet)) => Poll::Ready(Ok(packet)),
            TimedRecvOut::Timeout => {
                Poll::Ready(Err(NetworkError::Timeout(Some(this.link.sender()))))
            }
            TimedRecvOut::None => {
                // There is no packet available in the queue, hence you need to wait, i.e. park.
                switchboard.park(this.link, cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Suspension primitive on receive from *any* party, bounded by a timeout.
///
/// Like [`RecvAny`] it waits for the next packet from any of the `senders`, but gives up once the
/// recipient's virtual clock passes a deadline. The first poll schedules a timer on the
/// switchboard at that deadline, so the task is still woken to observe the timeout even if no
/// packet ever arrives; the future then resolves with the sender and its packet if one is
/// delivered in time, or with [`NetworkError::Timeout`] (carrying `None`, as no single party can
/// be blamed) once the deadline passes.
pub(crate) struct RecvAnyTimeout {
    /// The shared router the packet is awaited on.
    switchboard: Arc<Mutex<Switchboard>>,
    /// The receiving party.
    local: PartyId,
    /// The parties a packet is awaited from.
    senders: Vec<PartyId>,
    /// How long to wait for a packet, measured from the first poll, before timing out.
    timeout: Duration,
    /// Deadline for the timeout.
    ///
    /// Any time past this deadline is considered a timeout. This deadline is computed as the
    /// virtual instant at which the receive is first polled plus the timeout, and stays `None`
    /// until that first poll sets it.
    deadline: Option<Duration>,
}

impl RecvAnyTimeout {
    /// Creates a future that resolves with the next `(sender, packet)` that `local` receives
    /// from any party in `senders`, or with a timeout error if nothing arrives within `timeout`.
    pub(crate) fn new(
        switchboard: Arc<Mutex<Switchboard>>,
        local: PartyId,
        senders: Vec<PartyId>,
        timeout: Duration,
    ) -> Self {
        Self {
            switchboard,
            local,
            senders,
            timeout,
            deadline: None,
        }
    }
}

impl Future for RecvAnyTimeout {
    type Output = Result<(PartyId, Packet), NetworkError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut switchboard = this.switchboard.lock().expect("the lock must be free");
        let deadline = match this.deadline {
            Some(d) => d,
            // Here we are dealing with the first call to recv.
            None => {
                let deadline = switchboard.clock_of(this.local) + this.timeout;

                // A single timer suffices to observe the deadline: the task parks the same waker
                // on every inbound link, so a wake through any one of them re-polls this future.
                // The timer's link must be one of `senders`, as those are the links `park_any`
                // parks the waker on.
                if let Some(&sender) = this.senders.first() {
                    switchboard.schedule_timer(Link::new(sender, this.local), deadline);
                }

                this.deadline = Some(deadline);
                deadline
            }
        };

        match switchboard.try_recv_any_with_deadline(this.local, &this.senders, deadline) {
            TimedRecvOut::Some(result) => Poll::Ready(Ok(result)),
            TimedRecvOut::Timeout => Poll::Ready(Err(NetworkError::Timeout(None))),
            TimedRecvOut::None => {
                // There is no packet available in the queue, hence you need to wait, i.e. park.
                switchboard.park_any(this.local, &this.senders, cx.waker().clone());
                Poll::Pending
            }
        }
    }
}
