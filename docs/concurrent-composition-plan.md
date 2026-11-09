# scl-rs — Concurrent Protocol Composition: Implementation Plan

**Date:** 2026-10-07
**Against:** `main` at v0.14.2 (`6b898c5`).
**Status:** in progress (2026-11-09). Phase 0 is done (`38cc6e6`). Phase 1 is done in the working
tree, not committed: the session-keyed switchboard, `SimNetwork` sessions, `session` on the four
trace events, tests T3–T5 and the deadlock report (`SessionId` itself is committed, `e7cea51`).
Phase 2 is written and its tests are in `tests/concurrent.rs`; one of them fails until
`SimNetwork::fork` starts each child's fork count at 0. Phases 3–6 have not started.
**How this was checked:** the timing figures in §1 come from a throwaway simulator probe run against
v0.14.2. The trait and combinator signatures in §5.4–§5.5 were compiled against the v0.14.2 crate,
including the `Send` bound on the combined future, nested `Join`s and the type-erased variant.
Everything else is derived from reading the source; anything marked _(verify)_ should be confirmed
during implementation.
**Revised:** 2026-11-03, after a review against the working tree (`6b898c5` plus the uncommitted
`protocol::zk`). The revision fixed send cancel-safety and handle lifecycle on `TcpNetwork` (§5.3,
§5.8), the trace merge order (§5.6), the bystander case of I1 (§5.1), `Instant` links in the delay
split (§5.7), the Phase 5 acceptance criterion and the migration notes. It also reordered the
tests: each phase now writes its tests against a `todo!()` skeleton, instead of Phase 0 writing
them all up front (§7, §8). It did not re-run the §1 measurements or the §5.4–§5.5 compile check;
the compile check is now the first step of Phase 2.

---

## 0. Summary

Today every protocol composition in scl-rs is **sequential**. `Protocol::run`/`execute` take
`&mut E`, so two sub-protocols cannot be in flight at once. When they are independent, the simulator
measures the sum of their latencies instead of the maximum. A real `TcpNetwork` deployment cannot
overlap them either.

This plan adds **heterogeneous concurrent composition**: any number of independent protocols, of
any (possibly different) types, run concurrently within each party, and their typed outputs are
returned together:

```rust
let (prod, (x, doubles)) = Join((
    mul,                                    // PassiveShamirMul
    Join((rand_shr, rand_double_shr)),      // PassiveRandShr ‖ PassiveRandDoubleShr
)).execute(env).await?;
```

The feature rests on four pieces:

1. **Sessions.** Every concurrent branch gets a deterministic, hierarchical `SessionId`. Each
   message carries its session ID, and both backends route incoming messages by
   `(link, session)` instead of by `link` alone.
2. **Forking.** An `Environment` can be split into child environments (`ForkEnvironment::fork`).
   Each child owns a network handle bound to a child session and an RNG seeded from the parent's.
   Because each child is owned, the `&mut` problem disappears.
3. **Combinators.** `Join<(P1, …, Pk)>` (heterogeneous, typed tuple output), `JoinAll<P>`
   (homogeneous, `Vec` output) and `Boxed<E, O>` (type-erased, for a set of different protocols
   known only at runtime) are all ordinary `Protocol`s. They therefore nest, trace and compose
   like any other protocol.
4. **Observability and fidelity.** Traces and the bandwidth tree become session-aware. The delay
   model gains **link occupancy**, so concurrent messages on one link share its bandwidth instead
   of each getting all of it.

Code that never forks keeps its behaviour, its traces, its byte counts and (until Phase 5) its
virtual timings on the simulator. On `TcpNetwork` it sees two deliberate changes, listed under G6.

---

## 1. Motivation (measured on v0.14.2)

WAN preset (100 ms RTT, 100 Mbps), `n = 5`, `t = 2`, Mersenne61:

| Workload                                                                                                                                                         | Sequential today (measured) | With concurrency (critical path) | Bytes              |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------- | -------------------------------- | ------------------ |
| 2× `PassiveShamirMul` (1 product each)                                                                                                                           | 200 ms                      | 100 ms                           | 304 B (unchanged)  |
| Preprocessing in `tests/passive_shamir.rs::MulRandomPairs`: 4× `PassiveRandShr` (50 ms each) + 1× `PassiveRandDoubleShr` (100 ms), then `PassiveTriple` (100 ms) | 400 ms                      | 200 ms = max(50, 100) + 100      | 1428 B (unchanged) |

The sequential column was measured. The concurrent column is the critical path that concurrent
composition would produce; for the first row it equals the measured time of the batched
alternative.

**Measured on 2026-11-09** (test T2 (a), `tests/concurrent.rs`): the second row holds for every
party except the king, at 400.03 ms one after another and 200.02 ms under a `Join`. The king
takes 50 ms less in both, 350.03 ms and 150.01 ms, because `PassiveTriple` opens to the king,
which does not wait for the hop back.

The first row can already be fixed by **batching**: `PassiveShamirMul` with a vector of two products
also takes 100 ms. Batching remains the better tool whenever a protocol is vectorized, because it
sends one packet per link instead of one per instance. The second row cannot be fixed by
batching: the five sub-protocols have different types and different round structures. That is the
case this plan targets.

Sequential composition gets bytes right but **overestimates latency** by the ratio of the
sum of the latencies to the critical path. For a library whose distinguishing feature is calibrated
cost modelling, that is a fidelity bug as much as a missing feature.

---

## 2. Goals and non-goals

**Goals**

- G1. Run _k ≥ 2_ independent protocols concurrently within a party, with heterogeneous types and
  typed outputs. Nesting must be allowed to any depth.
- G2. Identical semantics on `SimNetwork` and `TcpNetwork` (the core thesis: one protocol
  description, two backends).
- G3. Deterministic and reproducible simulation, with the same seeds giving the same traces.
- G4. Correct virtual time: a party's elapsed time reflects the critical path, not the sum.
- G5. Sound per-protocol attribution in traces and bandwidth trees under interleaving.
- G6. No behaviour change for code that does not use the new combinators. There are two
  deliberate exceptions, both on `TcpNetwork` and both recorded in the CHANGELOG:
  - `recv_any` returns the lowest ready sender id, matching the simulator. Today it returns
    whatever order `StreamMap` yields (`tcp.rs:256-261`).
  - `send_to` returns once the frame is queued to the peer's writer task, not once it has been
    flushed (§5.3).

**Non-goals (for this plan)**

- CPU parallelism. Concurrency stays within each party's task; compute is not modelled (see the
  separate compute-cost roadmap item).
- Sharing a single uplink across _different_ links (NIC contention). This is already unmodelled
  for `send_many` and stays so; §5.7 models contention within one link only.
- Malicious or Byzantine use of session IDs:
  - binding them into transcripts, commitments and proofs;
  - validating received headers against the sessions a party actually runs;
  - bounding early-arrival buffering.

  `SessionId` already provides `digest()` for the binding (§5.1), but no protocol in this plan
  uses it. Under the honest-but-curious threat model a session ID is routing metadata, not an
  authenticated value.
- Assigning a session to every `execute` call. This is deferred; see decision C-2.

---

## 3. Why this is feasible without touching the executor or the clock

The executor (`net/simulation/executor.rs`) already drives arbitrary futures. The per-party virtual
clock is already compatible with concurrency inside a party:

- `deliver_next` runs only when **every** party task is parked, and pops events in global
  `(arrival, seq)` order (`switchboard/mod.rs:176`).
- A party's clock is `max(clock, arrival)` of what has been delivered to it.
- So when one branch of a party is woken by a delivery at `t₁`, no other delivery to that party
  with arrival `< t₁` is still pending: it would have popped first. Every send that branch makes is
  stamped at the earliest instant it could causally happen, whatever the other branches are doing.
- Local computation takes zero virtual time, so the order in which the branches are polled within
  one instant does not affect any timestamp.
- The existing argument for leftover timer events (`switchboard/mod.rs:181-184`) still holds,
  because "the party is parked" now means "all its branches are parked".

All the work is therefore in the API layer, the transport and the tracing. The scheduler and the
clock do not change.

---

## 4. Blockers on v0.14.2

| #   | Blocker                                                                                                                                         | Where                                               | Consequence if forced (e.g. by unsafe aliasing)                                                                                                                                                                               |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| B1  | `run`/`execute` take `&mut E`; `Network` methods take `&mut self`; `rng_mut` is exclusive                                                       | `protocol/mod.rs:95,111`; `net/mod.rs:452-518`      | `join!(a.execute(env), b.execute(env))` does not compile                                                                                                                                                                      |
| B2  | Messages are not tagged with a session; queues are keyed by `Link` only                                                                         | `switchboard/mod.rs:50`; `Packet`                   | The receiver takes the front of the link's queue. Correct only if, on every link, the order of sends across instances happens to match the order of receives. Same-typed payloads mean **silent wrong results**. Same on TCP. |
| B3  | One parked waker per `Link`; `try_recv_any` clears the wakers on _all_ inbound links                                                            | `switchboard/mod.rs:52,286-288`                     | A sibling's freshly parked waker gets deleted → lost wake-up → executor deadlock panic                                                                                                                                        |
| B4  | The trace parser treats begin/end markers as a stack; any `ProtocolEnd` closes the innermost node; `ProtocolId` can't distinguish two instances | `simulator.rs:324-349`; `simulation/mod.rs:211-238` | Interleaved branches nest wrongly; bytes are attributed to the wrong protocol                                                                                                                                                 |
| B5  | Delay is per message: `now + delay(size)` with only a FIFO clamp; no link occupancy                                                             | `switchboard/mod.rs:142-146`                        | Concurrent large messages on one link each get the full bandwidth → latency underestimated                                                                                                                                    |
| B6  | The RNG is a single `&mut R`                                                                                                                    | `protocol/mod.rs:152-159`                           | Concurrent branches need independent, deterministically derived streams                                                                                                                                                       |
| B7  | `TcpNetwork` owns its `StreamMap` readers and `PeerWriter`s by value                                                                            | `net/tcp.rs:86-94`                                  | No way to hand concurrent sessions independent receive paths; concurrent writes could interleave frame bytes                                                                                                                  |

---

## 5. Design

### 5.1 Sessions

```rust
// net/session.rs — implemented
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId {
    root_tag: [u8; 32], // identifies the execution: equal at every party, distinct across executions
    path: Vec<u32>,     // fork indices from the execution's root session; empty = root session
}

impl SessionId {
    pub fn root(root_tag: [u8; 32]) -> Self;
    pub(crate) fn new(root_tag: [u8; 32], path: Vec<u32>) -> Self; // frame decoding
    pub fn with_child(&self, index: u32) -> Self;
    pub fn parent(&self) -> Option<Self>;
    pub fn depth(&self) -> usize;
    pub fn root_tag(&self) -> &[u8; 32];
    pub fn path(&self) -> &[u32];        // what a message header carries
    pub fn digest(&self) -> [u8; 32];    // SHA-256 commitment to root tag + path
}
// Display (implemented): "root", "0", "0.3.1" — the path only, padded with `Formatter::pad`.
```

**The two parts and what each is for.** This follows the UC convention for session identifiers
(Canetti, *Universally Composable Security*, ePrint 2000/067): every protocol instance has its own
`sid`, all parties of an instance share it, distinct instances have distinct ones, and a
subroutine's identifier extends its caller's.

- **Root tag: unique across executions.** Without it, every execution restarts at the empty path,
  so two key generations or two epochs on the same parties would reuse the same identifiers. The
  tag is supplied from outside, for example a hash of the protocol name and version, the party set,
  the parameters, and an epoch counter or agreed randomness. Where the tag comes from is decision C-8.
  The general problem is treated by Barak, Lindell and Rabin, *Protocol Initialization for the
  Framework of Universal Composability* (ePrint 2004/006).
- **Path: unique within an execution.** It is assigned positionally by `fork` (below), with no
  communication. It is all that routing needs.
- **`digest()`** is SHA-256 over a domain-separation prefix, the root tag, the path length (as a
  `u64`) and the indices. The length prefix makes the encoding injective. It is the value to hash
  into transcripts, commitments and signed messages so they stay bound to their session. Nothing in
  this plan uses it yet (§2 non-goals).

**Representation choices:**
- **No serde derive.** The root tag is fixed per network instance, so a frame header carries only
  `path()`, and the receiver rebuilds the session with `SessionId::new`.
  - In the root session that header is 1 byte; the session `[1, 0]` is 3.
  - This keeps the 32-byte tag off the wire.
- **The path is a `Vec<u32>`.** Cloning an empty `Vec` allocates nothing, so code that never forks
  pays only a 32-byte copy of the tag per clone. A forked session pays one small allocation per
  clone, which is small next to what every message already allocates: the `Packet` clone in
  `send_to`, `Packet::composition`, and the event clone in `record_event`. Simulated timings are
  unaffected, since virtual time does not depend on allocation cost.
- **Comparison.** Equality, hashing and ordering compare the root tag, then the path. A session
  rebuilt from a frame therefore equals the one created locally by `fork`. Within one tag the
  ordering is depth-first.
- **The fields are private**, so the path can become an `Arc<[u32]>` later with no public API
  change, if the Phase 2 benchmark (§10) shows a reason.
- **The dependency on `sha2` 0.11.1 raises the MSRV to 1.88** (§9).

**Assignment rule.** Every network handle carries `session: SessionId` and `next_child: u32`.
`fork()` returns a handle for `session.with_child(next_child)` and increments `next_child`; the new
handle starts its own `next_child` at 0. Child IDs therefore follow the program order of `fork`
calls on each handle, with no communication and no global counter.

**Invariant I1 (symmetric forking).** Within a session, every party that communicates in it
performs the same sequence of forks. `Join`/`JoinAll` meet this automatically when all
participants execute the same protocol tree, which is how every scl-rs protocol is written today.
The invariant is broken by forking inside a role-dependent branch, for example:

- only the king forks;
- `JoinAll` with a party-dependent length;
- the "bystanders skip it" pattern, where only the participants execute a sub-protocol. This is
  already an idiom in the codebase: `SchnorrPok` says "every other party simply does not run it",
  and `tests/schnorr_pok.rs::MaybeProve` follows it.

If a `Join` is run under that last pattern, the bystanders' `next_child` falls behind the
participants'. From then on, **every later** fork in that session gets a different index on the
bystanders, not just the skipped one. The rule to document is therefore: _every party executes
every `Join`/`JoinAll`; a party that takes no part in a branch runs a branch that returns
immediately_. For example, wrap the sub-protocol the way `MaybeProve` does: its `None` arm
returns `Ok(None)` at once. Do not use `SchnorrPok` itself as a bystander's branch. It returns
`Error::Input` on a non-participant (`schnorr.rs:198-200`), and under fail-fast (§5.8) that would
abort the whole `Join`.

A violation shows up as messages stuck in a session that the recipient never opens. The simulator
turns this into a diagnosable deadlock (§5.2); on TCP it hangs until a timeout. The rule must be
documented on `fork`, `Join` and `JoinAll`.

**Invariant I2 (one waiter per session per party).** Handles are not `Clone`, and every receive
borrows the handle mutably. So for each `(party, session)` there is at most one receive in flight.
This lets both backends keep exactly one waker slot per `(link, session)`, which is the fix for B3.

**Sequential composition is unchanged.** A sub-protocol invoked with `.execute(env)` runs in its
caller's session, exactly as today. Only the combinators fork.

### 5.2 Transport: simulator

`SimNetwork` gains `session: SessionId` and `next_child: u32`. `SimNetwork::new` (its only caller
is `simulate`) creates the root handle, `SessionId::root(SIMULATION_ROOT_TAG)` (decision C-8, implemented).
`SimNetwork::new` and `simulate` keep their signatures.

`Switchboard` changes (all of it is `pub(crate)` except `clock_of`, so the change is internal):

| Field / fn                             | v0.14.2                           | New                                                                                |
| -------------------------------------- | --------------------------------- | ---------------------------------------------------------------------------------- |
| `msg_queues`                           | `HashMap<Link, VecDeque<Packet>>` | `HashMap<MailboxKey, VecDeque<Packet>>` with `MailboxKey { link, session }`        |
| `parked`                               | `HashMap<Link, Waker>`            | `HashMap<MailboxKey, Waker>`                                                       |
| `last_arrivals`                        | per `Link`                        | **unchanged, per `Link`**: sessions share one TCP stream, so FIFO stays per link   |
| `ScheduledEvent.link`                  | `Link`                            | `key: MailboxKey` (timers carry the session too)                                   |
| `send(from, to, packet)`               | —                                 | `send(from, to, session, packet)`                                                  |
| `try_recv*`, `park*`, `schedule_timer` | link-keyed                        | take `session`; `try_recv_any*` only removes wakers for **its own session's** keys |
| `deliver_next`                         | wakes `parked[link]`              | wakes `parked[key]`                                                                |

The `Recv`, `RecvAny`, `RecvTimeout` and `RecvAnyTimeout` futures carry the session. Messages that
arrive before the recipient has forked their session simply wait in a queue created on demand,
which is already how `entry().or_default()` behaves.

**Deadlock diagnostics.** `IdleOutcome::Deadlocked` carries a `DeadlockReport` built by
`Switchboard::deadlock_report()`, and the executor panic prints it. The report lists every
receive still waiting, as `(party, session)` and the senders it waits for, and every mailbox
with packets no receive took, as `(recipient, session, sender, #packets)`. Each mailbox line says
what its recipient is doing: waiting for these very packets (a lost wake-up, a simulator bug),
waiting in the same session for another sender (usually a protocol bug), waiting only in other
sessions (usually an I1 violation), or not waiting. This turns an I1 violation from "3 task(s)
parked" into "party 2 in session 0.4: 3 packet(s) from party 0; party 2 is waiting in other
sessions".

### 5.3 Transport: `TcpNetwork`

**Wire frame.** The length-delimited codec is unchanged. The frame body becomes
`postcard((path: &[u32], Vec<Element>))` instead of `postcard(Vec<Element>)`.
- **The session's root tag is not sent.** It is fixed for the `TcpCore` (decision C-8), and the
  reader task rebuilds each session with `SessionId::new(core_root_tag, path)`.
- **Header size:** 1 byte in the root session, 3 bytes for `[1, 0]`.
- **Compatibility:** this breaks wire compatibility with 0.14.x, like the 0.14.0 serialization
  change before it.

**Structure.** `TcpNetwork` becomes a cheap handle over a shared core:

```rust
pub struct TcpNetwork {
    local: PartyId,
    core: Arc<TcpCore>,
    session: SessionId,
    next_child: u32,
}

struct TcpCore {
    // One writer task per remote peer; a send is one enqueued, fully encoded frame.
    writers: HashMap<PartyId, mpsc::UnboundedSender<WriterCmd>>,
    mailboxes: Arc<std::sync::Mutex<Mailboxes>>,
    readers: Vec<tokio::task::AbortHandle>,
}

enum WriterCmd {
    Frame(Vec<u8>),                            // length prefix + header + body, already encoded
    Close(oneshot::Sender<Result<()>>),        // flush what is queued, shut down, ack
}

struct Mailboxes {
    queues: HashMap<(PartyId, SessionId), VecDeque<Packet>>,
    waiters: HashMap<SessionId, Waker>,      // one per session, by I2
    peer_state: HashMap<PartyId, PeerState>, // Open | Closed | Failed(NetworkError)
    retired: HashSet<SessionId>,             // dropped sessions; late frames are discarded
}
```

**Ownership.** The tasks never hold the `TcpCore`, so there is no reference cycle:

- A reader task owns its peer's `FramedRead` over the read half, plus a clone of the
  `mailboxes` `Arc`.
- A writer task owns its peer's write half, the `UnboundedReceiver<WriterCmd>`, and a clone of the
  `mailboxes` `Arc` (to record a write failure).

The core is dropped together with the last handle, root or forked (see _Lifecycle_).

- **Receive.** `TcpNetwork::create` spawns one reader task per remote peer, which wraps the
  existing `FramedRead` stream. Each task decodes frames, pushes them to
  `queues[(peer, session)]`, and wakes `waiters[session]`. The loop-back peer needs no channel:
  `send_to(self)` pushes straight into the mailbox.
- **Receive futures** have the same shape as the simulator's: lock, try to pop, otherwise register
  the waker and return `Pending`. `recv_any` picks the lowest ready sender id, matching the
  simulator. `recv_*_with_timeout` wraps the mailbox future in `tokio::time::timeout`. The pop and
  the `Ready` return happen in the same `poll`, so a dropped or timed-out receive never loses a
  packet. This is cancel-safe by construction and stronger than today's guarantee.
- **Peer failure.** When a reader task hits EOF or an error, or a writer task gets a write error,
  the task marks the peer `Closed`/`Failed` and wakes every waiter. After draining the queue, a
  receive from that peer returns `ConnectionClosed(Some(p))` or the stored error.
  `recv_any` returns `ConnectionClosed(None)` once every peer is closed and drained. A decode
  error cannot be attributed to a session, so it fails the whole peer.
- **Send.** `send_to` checks `peer_state` and returns the stored error if the peer has failed.
  Otherwise it encodes the complete frame (length prefix, session header and body) into one
  buffer, enqueues `WriterCmd::Frame` on that peer's channel, and returns the payload size. The
  writer task performs `write_all` and `flush` for each frame in queue order.
  - **No interleaving.** Frames from concurrent sessions can never interleave on the byte stream,
    because only the writer task writes to it.
  - **Cancel-safe.** Enqueueing is synchronous and all-or-nothing. Dropping a send future, which
    is exactly what fail-fast does to a sibling branch (§5.8), can therefore never leave a partial
    frame on a stream that other sessions share. A lock held across `write_all` would not be
    enough: tokio documents `AsyncWriteExt::write_all` as not cancellation safe.
  - **Behaviour change.** `send_to` now completes at enqueue, not at flush, and a write error
    surfaces at the next send to, or receive from, that peer (see G6).
  - **`send_many`** enqueues one frame per target and needs no `try_join_all`.
- **Lifecycle.**
  - _Forked handle._ Dropping a forked handle retires its session: it removes the session's queues
    and adds the session to `retired`. `close()` on a forked handle does the same.
  - _`close()` on the root handle._ Sends `WriterCmd::Close` to every writer and awaits the acks.
    Each writer flushes the frames queued ahead of the `Close`, shuts down its write half and
    exits. Then `close()` aborts the reader tasks.
  - _Dropping the last handle without `close()`._ This drops the `TcpCore`. Its `Drop` aborts the
    reader tasks and drops the writer channels. Each writer task then drains its queue, shuts down
    its write half and exits.

  Either way, both halves of every TLS stream are dropped, so peers see EOF. This preserves
  today's behaviour, where dropping a `TcpNetwork` closes its sockets (`tcp.rs:86-94`). A
  `JoinHandle` is not enough for this: dropping it detaches the task instead of aborting it. And
  `tokio::io::split` keeps the stream open while either half is alive.

**Why I/O tasks rather than demultiplexing inside the receiving task** (decision C-4):

- _Readers._ Tokio I/O keeps only the _last_ registered waker per direction. If two sessions take
  turns polling the same peer's stream, the session that registered first silently loses its
  wake-up: B3 again, on the real backend. One reader per peer makes that impossible.
- _Writers._ They make sends cancel-safe, as described above.

This reverses the roadmap §7 note that a task-per-peer design "was unnecessary — `StreamMap`
provides the multiplexing". That note was right for one consumer per stream. With several sessions
consuming one peer stream, it no longer holds, and the roadmap text is updated in Phase 6.

The "never `tokio::spawn`" rule in the `send_many` docs is about keeping the trait's _default_
methods drivable by the simulator. `TcpNetwork` already requires a tokio runtime, so spawning
inside it does not violate that rule. The doc comment should say this explicitly.

### 5.4 Forking API (compile-checked)

```rust
// net/mod.rs
pub trait ForkNetwork: Network + Sized {
    /// A handle on a fresh child session of this handle's session (rule in §5.1).
    fn fork(&mut self) -> Self;
    fn session(&self) -> &SessionId;
}

// protocol/mod.rs — a capability family, following the supertrait pattern already in use
pub trait ForkEnvironment: Environment + Sized {
    fn fork(&mut self) -> Self;
}

impl<N: ForkNetwork, R: CryptoRng + SeedableRng + Send> ForkEnvironment for GeneralEnv<N, R> {
    fn fork(&mut self) -> Self {
        GeneralEnv { network: self.network.fork(), rng: R::from_rng(&mut self.rng) }
    }
}
```

- `fork` returns `Self`, so a child environment has the **same type** as its parent. Every
  existing protocol therefore runs inside a branch unchanged, and `simulate`'s
  `E: Environment<Net = SimNetwork>` bound is untouched.
- It is a separate trait, so the ~50 existing `Protocol` impls keep their bounds. Only protocols
  that use the combinators add `E: ForkEnvironment`.
- RNG: the child is seeded from the parent CSPRNG at fork time (rand_core 0.10
  `SeedableRng::from_rng`). Forks happen in program order, so runs stay reproducible. Switching a
  composition from sequential to `Join` changes the random streams, so tests must compare
  reconstructed values, not shares.
- User environments that carry extra capabilities implement `ForkEnvironment` by hand, deciding
  per field whether it is shared (`Arc`), cloned, or derived.

### 5.5 Combinators (compile-checked)

New module `protocol::concurrent`:

```rust
/// Heterogeneous: a tuple of 2..=12 protocols of any types; output is the tuple of outputs.
pub struct Join<T>(pub T);
// macro-generated for each arity:
impl<E: ForkEnvironment, P0: Protocol<E>, P1: Protocol<E>> Protocol<E> for Join<(P0, P1)>
where P0::Output: Send, P1::Output: Send {
    type Output = (P0::Output, P1::Output);
    async fn run(self, env: &mut E) -> Result<Self::Output, Error> {
        let (mut e0, mut e1) = (env.fork(), env.fork());
        futures_util::try_join!(self.0 .0.execute(&mut e0), self.0 .1.execute(&mut e1))
    }
    fn id(&self) -> ProtocolId { ProtocolId::from("Join") }
}

/// Homogeneous: any number of instances of one protocol type; output is a Vec in input order.
pub struct JoinAll<P>(pub Vec<P>);   // forks one env per element, try_join_all

/// Type-erased: a protocol with its output mapped to a common type `O`, for heterogeneous sets
/// whose size is only known at runtime: `JoinAll<BoxedProtocol<E, O>>`.
pub struct BoxedProtocol<E, O>(Box<dyn DynProtocol<E, O>>);

// protocol/mod.rs — a provided method of `Protocol`
fn boxed<F, O>(self, map: F) -> BoxedProtocol<E, O>
where
    F: FnOnce(Self::Output) -> O + Send + Sync + 'static,
    Self: Sized + 'static;
```

- Use `futures_util`'s `try_join!` / `try_join_all` (already a dependency), **not** `tokio::join!`.
  Both poll in a deterministic order: fixed order, or wake order beyond 30 futures, and wake order
  is deterministic in the simulator.
- `DynProtocol` exists because `Protocol::run` returns `impl Future` and so cannot be used as a
  trait object. The adapter boxes the future (`BoxFuture<'a, Result<O, Error>>`). Callers usually
  map outputs into an enum.
- **Bounds (checked 2026-11-09):** the protocol and the map must be `'static`, because
  `Box<dyn …>` defaults to `+ 'static`; `E` need not be. An earlier version of this plan said the
  adapter needs `E: 'static`; it does not, and a test boxes protocols with a generic `E`.
  `Self: Sized` is needed because `boxed` has a default body that moves `self`, and `Self` in a
  trait is not `Sized` by default.
- **API (decided 2026-11-09):** `boxed` is a provided method of `Protocol` with a mandatory map,
  rather than a free function, and the type is `BoxedProtocol`. `DynProtocol` and its one
  implementation, which pairs the protocol with its map, are private.
- A boxed protocol reports the inner protocol's `id` and runs the inner `run`, not `execute`. When
  it is called with `execute`, the trace gets one scope, under the inner protocol's name.
- A closure-based form (`join_fn(env, |e| async { … })`) is **not** proposed. On the 1.88 MSRV
  there is no stable way to require `Send` on an `async` closure's future, and `Protocol` requires
  `Send`. The only workable form needs `for<'a> FnOnce(&'a mut E) -> BoxFuture<'a, _>`, which
  `Boxed` already covers.
- Guidance for the docs: **batch when the protocol is vectorized; `Join` when it is not or when the
  types differ.** Batching sends fewer packets.

Example, the `MulRandomPairs` preprocessing rewritten:

```rust
impl<E: RandEnvironment + ForkEnvironment> Protocol<E> for MulRandomPairs { …
    let (x, y, a, b, doubles) = Join((
        PassiveRandShr::<1, F>::new(T, parties())?,
        PassiveRandShr::<1, F>::new(T, parties())?,
        PassiveRandShr::<1, F>::new(T, parties())?,
        PassiveRandShr::<1, F>::new(T, parties())?,
        PassiveRandDoubleShr::<1, F>::new(T, parties())?,
    )).execute(env).await?;
    let triples = PassiveTriple::new(self.king, parties(), a, b, doubles)?.execute(env).await?;
```

### 5.6 Tracing and profiling

**Events.** `SendData`, `ReceiveData`, `ProtocolBegin` and `ProtocolEnd` gain
`session: SessionId`. The parent of a session is derivable from its path, so no separate "fork"
event is needed. Hooks receive the field for free; `MetricHook` already matches with `..`.

**Bandwidth tree** (`ProtocolBandwidthTree::parse`): rewritten as an arena build with one stack of
open nodes per session.

- `ProtocolBegin(s, name)`: the parent is the top of `open[s]`. If that is empty (the first
  protocol in a fresh branch), use the top of the nearest ancestor session's stack, which is the
  `Join` node. If that is empty too, use the root.
- `ProtocolEnd(s, name)`: pop `open[s]` and **check that the name matches**. This was impossible
  before; a mismatch makes the parser report a malformed trace.
- `SendData(s)`: attribute it to the top of `open[s]`, otherwise to the nearest ancestor's top.
- Unbalanced traces (a branch cancelled by fail-fast, §5.8, never records its `ProtocolEnd`):
  closing a node force-closes the open nodes of its descendant sessions.
- The public API and the folded output format are unchanged. Concurrent branches appear as
  children of a `Join`/`JoinAll` frame, e.g. `<simulation>;Root;Join;PassiveRandShr 96`.

**`SimulationTrace` `Display`.** This is rendered from the same tree. Every arena entry stores the
index of its event in the raw trace. Each node prints its own events and its child blocks merged
by that **record index**, not by timestamp. Timestamp ties are the norm, because local computation
takes zero virtual time, so ordering by timestamp would reorder simultaneous events. Merging by
record index makes a purely sequential trace render **byte-identically** to v0.14.2, whose renderer
walks the raw order (`simulation/mod.rs:211-238`); a golden test pins this. Branches of a `Join` are printed one
after another, each labelled with its session (`PassiveRandShr [session 0.2] {`); their timestamps
show the overlap. `SimulationTrace::events()` stays the raw chronological list. Add
`SimulationTrace::session(&SessionId)` to filter one branch.

**Stretch goal (feeds the latency-metrics roadmap item):** have the tree nodes record begin/end
timestamps, giving a per-protocol span. Under concurrency spans no longer add up (a `Join`'s span is
the max of its branches'), so this is where the sim can display the critical path.

### 5.7 Timing model: link occupancy

`Delay` gains a method with a default, so the change is non-breaking:

```rust
pub trait Delay: Send {
    fn delay(&self, link: Link, size_bytes: usize) -> Duration;
    /// (serialization, propagation). Default: no serialization term → today's behaviour.
    fn transmission(&self, link: Link, size_bytes: usize) -> (Duration, Duration) {
        (Duration::ZERO, self.delay(link, size_bytes))
    }
}
```

`ConfigDelay` overrides it by splitting `ChannelConfig::recv_time_tcp` (`channel.rs:233`) into
`total_size_bits / throughput` and `rtt / 2`. For `NetworkType::Instant` it returns `(0, 0)`,
mirroring `message_delay`. `message_delay` remains the sum of the two terms. The switchboard keeps
`busy_until: HashMap<Link, Duration>`:

```
start      = max(now, busy_until[link])
busy_until = start + serialization
arrival    = busy_until + propagation          // still clamped by last_arrivals (FIFO)
```

This models one TCP stream per link, which is exactly what concurrent sessions share. It also
changes timings for _existing_ code that sends back-to-back on one link at the same instant; those
are microseconds for small messages. So it ships separately (Phase 5). It is validated against
`benches/comparison` with a new two-concurrent-transfers `tc netem` scenario and noted in the
CHANGELOG as a fidelity fix (precedent: 0.11.2).

**Acceptance criterion.** The harness has no pass/fail tolerance today. It reports the signed
relative error of the simulation against the real median and mean. So the criterion is stated
relative to existing numbers, following C-6. On the concurrent scenario:

1. With occupancy, the absolute relative error against the real median is no larger than the
   `BulkTransfer` error in the same regime. The single-transfer error is the model's baseline.
2. With occupancy, that error is smaller than without occupancy, i.e. occupancy closes the gap
   rather than widening it.

If either fails, occupancy ships opt-in (C-6) and the result is recorded in the README.

### 5.8 Errors and cancellation

- **Fail-fast** (decision C-3): the first branch to return `Err` makes the combinator return it, and
  the sibling futures are dropped.
- Simulator: dropped siblings leave stale wakers (a harmless extra poll at worst) and unconsumed
  mailboxes (which show up in deadlock reports only). `simulate` still panics through
  `drive_party_to_completion`'s `expect`, as today.
- TCP: peers blocked in a dropped session wait until their own timeout or error. That matches what
  happens today when any protocol fails part-way. Dropping a sibling's send cannot corrupt the
  stream that other sessions share, because a send is a single synchronous enqueue (§5.3). The
  parent session therefore stays usable after a combinator returns `Err`.
- An `Err` returned by `run` still records `ProtocolEnd` (because `execute` records it
  unconditionally). Only a _drop_ leaves the trace unbalanced, which §5.6 handles.

---

## 6. Open decisions

| #   | Question                          | Options                                                                                                | Recommendation                                                                                                                                                                                                                                                   |
| --- | --------------------------------- | ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C-1 | How sessions are assigned         | positional counter / explicit labels (`fork_labeled("dkg")`)                                           | Positional counter. Labels can be added later as a debug-only annotation on top of the path.                                                                                                                                                                     |
| C-2 | Where sessions are created        | only in combinators / every `execute` gets its own session                                             | Combinators only. Per-`execute` sessions would also isolate leftover messages from quorum receives in _sequential_ code (a latent hazard today), but they change the wire behaviour of every protocol and require symmetric `execute` trees across roles. Defer. |
| C-3 | Error semantics                   | fail-fast / settle-all (`(Result<O1>, Result<O2>)`)                                                    | Fail-fast (`try_join`). Add `JoinSettled` later if a protocol needs it (e.g. a best-effort branch).                                                                                                                                                              |
| C-4 | TCP I/O                           | one reader and one writer task per peer / demultiplex inside the receiving task and write under a lock | One reader and one writer task per peer (rationale in §5.3): readers avoid lost wake-ups; writers make sends cancel-safe. Reverses the roadmap §7 "`StreamMap` suffices" note, which assumed one consumer per stream.                                            |
| C-5 | Session header in cost accounting | payload-only / include header in the delay model                                                       | Payload-only everywhere in 0.15.0, so sequential timings stay bit-identical. The header is 1–4 bytes against 40-byte TCP/IP headers that are already modelled. Revisit with Phase 5.                                                                             |
| C-6 | Link occupancy                    | on by default / opt-in                                                                                 | On by default, but only after the `benches/comparison` validation shows it closes, rather than widens, the gap between simulated and real timings.                                                                                                               |
| C-7 | Names                             | `Join`/`JoinAll` / `Par`/`ParAll` / `Concurrent`                                                       | `Join`/`JoinAll`, mirroring `futures`.                                                                                                                                                                                                                           |
| C-8 | Source of the root tag (§5.1) — **decided and implemented 2026-11-03** | (a) simulator: a fixed tag inside `simulate`, signature unchanged; (b) simulator: a tag parameter on `simulate`; (c) TCP: a new `root_tag` parameter on `TcpNetwork::create`; (d) TCP: an optional `root_tag` field in the JSON `NetworkConfig`; (e) TCP: agreed at connection time by a nonce exchange (ePrint 2004/006) | **(a) + (c), caller-supplied, no protocol.** The simulator uses the constant `SIMULATION_ROOT_TAG`: one `simulate` call is one execution, so a fixed tag cannot collide. `TcpNetwork::create(id, config, root_tag)` takes a *required* tag with no default, since silent reuse is the failure to prevent. Callers derive it from public context with `derive_root_tag(context)` (protocol name and version, party set, parameters, and an epoch the application never reuses). (b) buys nothing; (d) invites reuse, because config files outlive runs; (e) was ruled out. Today the tag only feeds `digest()`: frames carry only the path, and executions use separate connections, so a reused tag cannot mix messages. |

---

## 7. Phases

Dependencies: 0 → 1 → 2 → {3, 4} → 6; 5 depends only on 1.

**Working rule for Phases 1–5: skeleton, then failing tests, then implementation.**

1. **Skeleton.** Write the phase's new types, traits and signatures, with `todo!()` bodies wherever
   the logic is not written yet. The goal is that the phase's tests compile.
2. **Failing tests.** Write the phase's tests from §8. They compile against the skeleton and fail
   when run, typically by panicking on a `todo!()`.
3. **Implementation.** Implement until they pass.

The acceptance tests cannot be written all at once up front. `#[ignore]` only stops a test from
running; it still has to compile, so a test cannot exist before the types it names. The §8
*Phase* column says in which phase each test is written and must pass.

### Phase 0 — Golden snapshot (no library change)

- **Golden snapshot** (`tests/golden.rs`, baselines in `tests/golden/<scenario>.txt`). It needs no
  new API, and it must exist before any library change, because it is what proves G6 phase by
  phase.
- **Scenarios:**
  - the seven simulator examples, with their protocols copied into the test, since tests cannot
    import code from examples;
  - the timing-sensitive shapes from `tests/simulator.rs` and `tests/recv_timeout.rs`:
    bandwidth-bound transfer, quorum with a leftover message, straggler delivered after quorum,
    and timed receives (a fired timer, a stale timer, a late packet left queued).
- **Captured per scenario:** the `metrics()` table and, per party in `PartyId` order:

  | Capture | Phase it protects |
  | --- | --- |
  | the trace `Display` | 1, 4 |
  | the **exact** timestamp of every event, in nanoseconds | 1, 5 |
  | the folded bandwidth tree (`bandwidth_tree_for` + `write_folded`) | 4, which rewrites `ProtocolBandwidthTree::parse` |

  Exact timestamps are required because the trace and the metrics table both round to the
  millisecond (`event.rs:258,260`, `simulator.rs:161`), so a sub-millisecond change, which is the
  scale Phase 5 introduces, would be invisible in them.
- **Determinism:** every RNG is seeded (`ChaCha20Rng::seed_from_u64`, a distinct seed per party and
  scenario), and the examples' random inputs are replaced by fixed ones. The examples seed from OS
  entropy, so their printed output cannot serve as a baseline. Example protocols that sampled from
  a fresh OS-seeded RNG inside `run` draw from the environment's RNG instead. Protocol ids, message
  sizes and message patterns stay as in the examples.
- **Regenerate** only for an intended change: `SCL_UPDATE_GOLDEN=1 cargo test --test golden`, then
  review the diff of `tests/golden/`.
- **Exit:**
  - the baseline is generated on a clean commit;
  - it passes in two further independent runs;
  - it is shown to fail on a deliberate small change and to pass again once reverted;
  - it is committed.
- Size: S.

### Phase 1 — Sessions in the simulator (internal, behaviour-preserving)

- **Skeleton:** `SessionId` with its methods; the session-taking signatures of
  `Switchboard::send`, `try_recv*`, `park*` and `schedule_timer`.
- **Tests:** T3, T4 and T5 as in-crate unit tests in `switchboard/mod.rs`. Those functions are
  `pub(crate)`, so the tests call `send`/`try_recv*`/`park*` directly with explicit sessions, and
  need no fork API. This covers the session routing and the lost-wake-up fix as soon as they are
  written, instead of leaving them untested until Phase 2.
- `net/session.rs` with `SessionId` (+ prelude export).
- `SimNetwork { session, next_child }`; session-keyed `Switchboard` per §5.2; session-carrying
  `Recv*` futures and `ScheduledEvent`.
- `Event::{SendData, ReceiveData, ProtocolBegin, ProtocolEnd}` gain `session`. This breaks every
  struct-literal construction and every pattern without `..`. Inside the crate those are the
  constructions at `switchboard/mod.rs:132,216,243` and `network.rs:119,131`; in tests, the
  constructions at `tests/simulator.rs:361,367`. The `event.rs` `Display` arms (`:205,214`) and
  `Event::timestamp` already use `..` and do not break.
- Deadlock report (§5.2).
- **Exit:** the Phase 0 golden snapshot passes unchanged (everything runs in the root session);
  the T3–T5 unit tests pass.
- Size: M.

### Phase 2 — Forking and combinators (simulator end-to-end)

- **Skeleton:**
  - `ForkNetwork` and `ForkEnvironment`, with `todo!()` in `SimNetwork::fork`/`session`.
  - The `ForkEnvironment` impl for `GeneralEnv`; it is one line, so write it in full.
  - `Join`, `JoinAll` and `BoxedProtocol`/`boxed`. Their `run` bodies are a few lines, so write
    them in full; the `todo!()` sits in `fork` underneath.

  First check that the skeleton compiles. That confirms the plan's §5.4–§5.5 claims. **Checked
  2026-11-09:** nested `Join` futures are `Send`; boxing needs the protocol and the map to be
  `'static`, not `E` (§5.5).
- **Tests:** T1, T2, T7 and T10, plus T3, T4 and T5 end to end through `Join` on the simulator.
- `ForkNetwork` and its impl for `SimNetwork`; `ForkEnvironment` and its impl for `GeneralEnv`.
- `protocol::concurrent`: `Join` for arities 2..=12 (macro), `JoinAll`, `BoxedProtocol`/`boxed`.
- Prelude: `ForkEnvironment`, `Join`, `JoinAll`.
- **Exit:** T1, T2, T3, T4, T5, T7 and T10 pass on the simulator.
- Size: M.

### Phase 3 — `TcpNetwork` sessions

- **Skeleton:** the `ForkNetwork` impl for `TcpNetwork` with `todo!()` bodies. The public
  signatures of `create`, `send`, `recv` and the `Network` methods do not change.
- **Tests:** T3 and T5 over TCP, and T8.
- Frame format, `TcpCore`, reader and writer tasks, mailboxes, `ForkNetwork` impl, lifecycle
  including `TcpCore`'s `Drop` (§5.3). Public API kept: `create` (already takes `root_tag`, C-8), `send`, `recv` and every
  `Network` method.
- **Exit:** T3, T5 and T8 pass over real mTLS loopback; the existing inline `tcp.rs` tests pass
  unchanged.
- Size: L. This is the riskiest phase: concurrency in real I/O plus lifecycle.

### Phase 4 — Session-aware tracing

- **Skeleton:** `SimulationTrace::session(&SessionId)` with a `todo!()` body.
- **Tests:** T6.
- Arena-based `ProtocolBandwidthTree::parse` with per-session stacks, name checking and forced
  closing (§5.6); tree-rendered `SimulationTrace` `Display`; `SimulationTrace::session`.
- **Exit:** T6 passes; the golden trace renderings for sequential examples are byte-identical.
- Size: M.

### Phase 5 — Link occupancy

- **Skeleton:** `Delay::transmission` with its default body (the default is today's behaviour, so
  it is real code, not a stub), and `ConfigDelay::transmission` with a `todo!()` body.
- **Tests:** T9.
- `Delay::transmission` with its default; `ConfigDelay` split; `busy_until` in the switchboard.
- New `benches/comparison` scenario: two concurrent large transfers on one link, simulated vs
  `tc netem`.
- **Exit:** T9 passes; the concurrent scenario meets the §5.7 acceptance criterion (otherwise
  occupancy ships opt-in); the golden-snapshot diffs are reviewed and explained in the CHANGELOG.
- Size: S–M.

### Phase 6 — Examples, docs, roadmap

- `examples/concurrent_preprocessing.rs` prints `metrics()` for the sequential and concurrent forms
  side by side (the §1 table, reproduced).
- Port `secure_covariance` preprocessing and `MulRandomPairs` to `Join` where the steps are
  independent.
- Module docs for `protocol::concurrent` covering I1, I2, batch-vs-join guidance and fail-fast.
- Roadmap: add a §7 entry and the §12 release rows (Appendix A). Update the §7 `StreamMap` note
  to point at C-4.
- README / crate docs: update the "untested axis is concurrency" paragraph (`lib.rs:366-369`) with
  the Phase 5 result.
- Size: S.

---

## 8. Test matrix

| ID  | What                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Backend   | Phase |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------- | ----- |
| T1  | Correctness: `Join((RandShr, RandDoubleShr, Mul))`, `JoinAll` of 8 `PassiveShamirMul` on independent triples, nested `Join` 3 levels deep. Outputs reconstruct to the expected values.                                                                                                                                                                                                                                                                      | sim       | 2     |
| T2  | Timing: the §1 preprocessing takes ≈200 ms under `Join` vs ≈400 ms sequential (equal to the critical path up to the serialization terms) (WAN preset). **Property test** (proptest): random composition trees of k-round leaf protocols over constant-delay links. Elapsed time equals the critical path (sequential = sum, `Join` = max).                                                                                                                  | sim       | 2     |
| T3  | Isolation under adversarial ordering: branch B sends before branch A on the same link while the receiver awaits A first. Each branch gets its own packets. This is the case that silently corrupts on v0.14.2.                                                                                                                                                                                                                                              | sim + TCP | 1, 2, 3 |
| T4  | Lost wake-up regression: `recv_any` in session A and `recv_from` in session B, both waiting on the same link, in both poll orders.                                                                                                                                                                                                                                                                                                                          | sim       | 1, 2  |
| T5  | Early arrival: a peer sends on session `0.3` before the receiver has forked it; the message is buffered, then consumed.                                                                                                                                                                                                                                                                                                                                     | sim + TCP | 1, 2, 3 |
| T6  | Tracing: in a `Join` run, each branch's bytes in the tree equal the bytes of that protocol run alone; `ProtocolEnd` name mismatch detected; a cancelled branch is force-closed; sequential renderings are byte-identical to v0.14.2.                                                                                                                                                                                                                        | sim       | 4     |
| T7  | Determinism: same seeds → identical `Event` lists across two runs, including `JoinAll` with more than 30 children (the `FuturesOrdered` path).                                                                                                                                                                                                                                                                                                              | sim       | 2     |
| T8  | Real network: 3-party mTLS loopback running `Join` of two different protocols; a timed-out receive on one session followed by a later successful receive (receive cancel-safety); a `Join` that fails fast while a sibling is sending, followed by a successful exchange in the parent session (send cancel-safety); peer EOF reaches waiters in every session; dropping the root handle without `close()` makes peers' receives return `ConnectionClosed`. | TCP       | 3     |
| T9  | Occupancy: two concurrent 1 MiB messages on one link → the second arrives at ≈ 2·serialization + propagation; concurrent messages on distinct links are unaffected.                                                                                                                                                                                                                                                                                         | sim       | 5     |
| T10 | Violating I1 → the deadlock panic names the orphaned session and the party holding its packets. Two shapes: fork only on the king; and bystanders skipping a `Join` and then all parties running a second `Join`. In the second shape, the second `Join` is the one that deadlocks.                                                                                                                                                                         | sim       | 2     |

The *Phase* column is the phase in which a test is written (failing, against that phase's
skeleton) and must pass (see the working rule in §7). A test listed under several phases is
written once per phase, against that phase's API:

- **Phase 1:** in-crate unit tests in `switchboard/mod.rs`, with explicit sessions.
- **Phase 2:** end to end through `Join` on the simulator.
- **Phase 3:** over real mTLS.

---

## 9. Release mapping and migration

| Version    | Contents                                                                                                               | Breaking?                                                                                                                                                                                 |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **0.15.0** | Phases 1–4 (+ 6 docs/examples): sessions on both backends, fork traits, `Join`/`JoinAll`/`Boxed`, session-aware traces | Yes: new `Event` fields; TCP wire format; TCP `recv_any` order and `send_to` completion point (G6); the default `Display` of a trace changes for concurrent runs only; MSRV 1.85.1 → 1.88 (`sha2` 0.11.1); `TcpNetwork::create` gains a required `root_tag` (C-8). Minor bump per §2. |
| **0.15.1** | Phase 5 link occupancy                                                                                                 | No API break (default trait method); timings for same-link back-to-back sends change. Patch, following the 0.11.2 precedent.                                                              |

Phases 1–4 ship **together**. Releasing combinators that work on the simulator but not on
`TcpNetwork` would break the core thesis (G2).

**Migration notes for 0.15.0**

- `Event::{SendData, ReceiveData, ProtocolBegin, ProtocolEnd}`:
  - patterns without `..` need `session` or `..`;
  - struct literals, e.g. a hand-built trace passed to `SimulationTrace::new`, need a `session`
    field (`SessionId::root(tag)` for sequential traces).
- Mixed 0.14.x / 0.15.0 deployments cannot talk to each other (frame format).
- `TcpNetwork::create` takes a third, required argument: the execution's root tag. Derive it with
  `derive_root_tag`, including an epoch or run counter that is never reused (C-8).
- The MSRV is now 1.88, required by the new `sha2` dependency of `SessionId::digest`.
- `TcpNetwork::recv_any` now returns the lowest ready sender id. `send_to` returns once the frame
  is queued, and a write error is reported by the next operation on that peer.
- Run every `Join`/`JoinAll` on every party (I1, §5.1). A protocol that let bystanders skip a
  sub-protocol must give them a branch that returns immediately instead.
- Custom environments: implement `ForkEnvironment` only if you want to use the combinators.
- Custom `Network` implementations: unaffected unless they want to support forking (`ForkNetwork`).

---

## 10. Risks

| Risk                                                                                                                                                                                                         | Mitigation                                                                                                                                                         |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| I1 violated silently on TCP (hang)                                                                                                                                                                           | Document it on `fork`/`Join`; the sim deadlock report catches it in tests; recommend developing on the sim first, which is already the intended workflow.          |
| TCP I/O tasks have no back-pressure on either side: readers drain sockets eagerly, so a fast sender grows receiver memory; `send_to` completes at enqueue, so a fast sender also grows its own writer queues | Acceptable under the honest-but-curious model and research workloads. Note it in the docs. Bounded mailboxes and bounded writer channels can come later if needed. |
| `retired` session set grows over long runs                                                                                                                                                                   | Prune with a per-parent high-water mark (child indices are monotone) if it ever matters.                                                                           |
| Phase 5 shifts published numbers (README fidelity claims)                                                                                                                                                    | Rerun `benches/comparison`; update the README figures in the same release.                                                                                         |
| Trace `Display` rewrite regresses sequential output                                                                                                                                                          | Byte-identical golden test from Phase 0.                                                                                                                           |
| `SessionId` cloning and hashing per packet: a 32-byte tag copy plus, in forked sessions, one small allocation for the path | Negligible next to the per-message `Packet` clone, `composition()` and serialization; affects wall-clock simulator speed only, never virtual time. Benchmark once in Phase 2; if it matters, make the private path an `Arc<[u32]>` (no API change). **Benchmarked 2026-11-09** (release build, one run, one machine, not part of the repo): a clone takes about 4 ns in the root session and about 10 ns at depth 1 or 3, against about 60 ns for cloning a 5-element `Packet` and about 124 ns for its `composition()`. A packet clones its session a few times, so this stays below the costs it already pays; the path stays a `Vec<u32>`. |
| Root tag reused across executions (C-8) | Same paths in two executions then name the same sessions, and any `digest()` binding is void across them. Mitigated by C-8: the TCP tag is a required parameter with no default, and `derive_root_tag` docs tell callers to include a never-reused epoch. Routing is unaffected either way (separate connections; frames carry only the path). |

---

## 11. Out of scope / follow-ups

- Per-`execute` sessions (C-2), which would also harden sequential composition against straggler
  messages.
- `JoinSettled` (settle-all semantics).
- Uplink/NIC contention across links in the delay model.
- Per-protocol span and critical-path reporting built on the Phase 4 tree (the latency and
  round-count metrics roadmap item).
- Compute-cost modelling. Concurrency makes it more valuable: overlapping communication with
  compute is the next fidelity gap.

---

## Appendix A — Suggested roadmap text

**§7 entry**

> - [ ] **Concurrent (heterogeneous) protocol composition.** Independent sub-protocols currently run
>       sequentially because `Protocol::run` borrows `&mut E`, so the simulator reports the sum of their
>       latencies instead of the critical path (e.g. `MulRandomPairs` preprocessing: 400 ms vs 200 ms
>       on WAN). Plan in `docs/concurrent-composition-plan.md`: deterministic hierarchical session IDs on
>       every message, routed by `(link, session)` on both backends; `ForkEnvironment::fork` (child
>       network handle and RNG seeded from the parent); `Join`/`JoinAll`/`Boxed` combinators as ordinary
>       `Protocol`s; session-aware traces and bandwidth trees; link occupancy in the delay model.

**§12 rows**

> | **0.15.0** | _Concurrent composition_ | Sessions on both backends, `ForkEnvironment`, `Join`/`JoinAll`/`Boxed`, session-aware traces. Breaking (`Event` fields, wire format). |
> | **0.15.1** | _Link occupancy_ | Concurrent messages on one link share its bandwidth; validated against `benches/comparison`. Timing fix, patch per §2. |
