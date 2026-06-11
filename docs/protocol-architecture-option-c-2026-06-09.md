# scl-rs Protocol Architecture — Option C (Async-as-Coroutine + Deterministic Executor)

**Date:** 2026-06-09
**Status:** In progress — Steps 1–5b implemented: the deterministic core has full output / trace /
hook parity, the test suite is migrated, and the **legacy tokio simulator has been deleted** (the
new core is the only simulator). Steps 6–7 and remaining follow-ons pending.
**Scope:** Redesign of how MPC protocols are modeled, composed, and simulated in
`scl-rs`, moving from a tokio-driven simulator to a single-threaded deterministic
executor that drives `async` protocols written once and run unchanged on both the
simulator and real TCP/TLS.

This document is the design record for the change. It captures the motivation, the
options considered, the rationale for the chosen path, every implementation step
(done and planned), the key concepts a contributor needs, the decisions made along
the way, and future strategies. It is written to be self-contained: someone reading
it cold should be able to understand and continue the work.

---

## Table of Contents

1. [Motivation and goals](#1-motivation-and-goals)
2. [Options considered](#2-options-considered)
3. [Why Option C, and two decisive questions](#3-why-option-c-and-two-decisive-questions)
4. [The step roadmap](#4-the-step-roadmap)
5. [Step 1 — Drive one future by hand (`block_on`)](#5-step-1--drive-one-future-by-hand-block_on)
6. [Step 2 — The N-task deterministic scheduler](#6-step-2--the-n-task-deterministic-scheduler)
7. [Step 3 — The suspension primitive (`Recv` + `Switchboard`)](#7-step-3--the-suspension-primitive-recv--switchboard)
8. [Step 4 — The virtual-clock event loop](#8-step-4--the-virtual-clock-event-loop)
9. [Step 5 — Running a real `Protocol` on the core](#9-step-5--running-a-real-protocol-on-the-core)
10. [Step 6 — Real async TCP/TLS (planned)](#10-step-6--real-async-tcptls-planned)
11. [Step 7 — Typed sub-protocol composition (planned)](#11-step-7--typed-sub-protocol-composition-planned)
12. [Key concepts reference](#12-key-concepts-reference)
13. [Decisions log](#13-decisions-log)
14. [Design invariants to preserve](#14-design-invariants-to-preserve)
15. [Future strategies and open questions](#15-future-strategies-and-open-questions)

---

## 1. Motivation and goals

`scl-rs` is a Rust port of Anders Dalskov's
[secure-computation-library (SCL)](https://github.com/anderspkd/secure-computation-library).
The original protocol model was ported faithfully from SCL's C++:

- `Protocol::run(&self, env)` returns a `ProtocolResult { result_bytes, next_protocol }`
  — a **trampoline / continuation** mechanism (a protocol ends and names its successor).
- Protocols perform I/O directly through an injected `Network` trait
  (`env.network.send_to(...).await` / `recv_from(...).await`).
- A discrete-event **simulator** runs each party as a tokio task, with timing
  derived analytically from network parameters (RTT, bandwidth, loss).

We want the architecture to follow MPC composition principles more faithfully:

- **Protocols compose**: a protocol can be *called within* another protocol, with a
  **typed return value** (not just `Vec<u8>`).
- **A protocol can output a result (bytes) or run another protocol** (the existing
  `ProtocolResult` already models this well; keep it for phase sequencing).

### Where the ported model falls short

- The trampoline only supports **tail/sequential** composition at the driver level.
  It cannot express **call-and-return nesting** with data flowing back, because a
  sub-protocol's output is `Vec<u8>` and bypasses the driver's event recording.
- `run(&self, ...)` is immutable-`self`, so a protocol can't accumulate state across
  phases except by returning a new `Box` via the trampoline.
- The simulator must **fake** discrete-event scheduling on top of tokio: the
  `recv` path spins on `tokio::task::yield_now()`, and `SimulatedChannel::has_data`
  *guesses* peer state via `remote_ahead || remote_dead || remote_receiving`
  (`src/net/simulation/channel.rs`). This is fragile and hard to reason about.

---

## 2. Options considered

**Option A — Refine the current async + `Network` DI.** Keep async/await; add a typed,
nestable `SubProtocol` abstraction beside the trampoline. Cheapest; solves
composition. Does **not** fix simulator fragility (still tokio-driven).

**Option B — Sans-IO (pure state machines).** Protocols become transport-free state
machines (`on_message`/`poll` returning `Action`s); the runner does all I/O. Maximal
decoupling, trivial adversarial testing, simplest simulator. **But** MPC protocols are
deeply sequential (many rounds); a state machine chops straight-line "send/recv/reconstruct"
code into explicit states — a steep, recurring ergonomic tax for a library whose main
activity is *writing new protocols*.

**Option C — Async-as-coroutine with our own deterministic executor.** Protocols stay
straight-line `async` code, but we **stop running them on tokio** for simulation.
`Network::recv_from` returns a future that, on first poll, registers "party *i* is
blocked on a recv from *j*" and returns `Poll::Pending`. The simulator **is** the
executor: it polls each party, learns exactly who is blocked, advances a virtual clock
to the next deliverable event, delivers, and re-polls. This gives **Sans-IO's
semantics (explicit blocking state) with async/await's ergonomics**.

| | A: Refine async | B: Sans-IO | C: Async + own executor |
|---|---|---|---|
| Protocol readability | ✅ straight-line | ❌ state machines | ✅ straight-line |
| Nesting + typed returns | ✅ (add SubProtocol) | ✅ | ✅ |
| Deterministic sim | ⚠️ tokio-driven | ✅ | ✅ |
| Fixes `has_data`/yield fragility | ❌ | ✅ | ✅ |
| Adversarial/reorder testing | ⚠️ hard | ✅ easy | ✅ easy |
| Implementation cost | Low | High (rewrite protocols) | Medium (write executor) |

**The reframing that settled it:** Sans-IO's real value to us is **not** "no I/O in
protocols" — we already inject I/O via the `Network` trait. Its value is making each
party's *blocking state explicit* so the simulator stops guessing. Option C buys
exactly that while keeping async ergonomics, and (Option A's composition win can be
layered on top in Step 7).

Two sub-strategies within C:
- **C1** — roll our own minimal executor; forbid tokio inside protocols. Full control,
  honest determinism contract. **← chosen.**
- **C2** — use a deterministic tokio-compatible shim (e.g. `madsim`). Less code, but a
  heavy dependency and less control over scheduling. Rejected for a library whose value
  proposition is a *precise* simulator.

---

## 3. Why Option C, and two decisive questions

### Q1 — Can one protocol run unchanged on both the simulator and real TCP/TLS?

**Yes — this is the headline benefit, and it is fundamental, not a trick.** An
`async fn` is executor-agnostic: a `Future` has no idea who polls it. The same protocol
future, generic over `N: Network`, is driven by:

- **Real deployment:** tokio (or smol); `recv_from` is a genuine async socket read with TLS.
- **Simulation:** our deterministic executor; `recv_from` is the suspending in-memory future.

**The one rule that makes this hold:** a protocol may only **suspend** (`.await`) through
abstractions that *both* executors implement — in practice `Network` and a virtual-aware
`Clock`. Suspension through a tokio-only primitive (a raw `tokio::time::sleep`,
`tokio::fs`, an external async HTTP client) would have no one to wake it under the
deterministic executor. This is the same discipline that already makes `N: Network`
swappable, extended to time.

### Q2 — Is cutting tokio from protocols a big deal for users?

**Mostly no — and what you lose is mostly a guardrail.** Separate two layers:

- **Runner / deployment layer** keeps tokio entirely (real `TcpNetwork`, TLS handshake).
- **Protocol-author layer** is the only one constrained. What protocols actually need:
  - *Sequential send/recv* → `Network`. No tokio.
  - *Concurrency* ("send to all, recv from all") → the `futures` crate combinators
    (`join!`, `select!`, `try_join_all`, `FuturesUnordered`) are **executor-agnostic**
    and work under any executor. Concurrency does **not** require tokio.
  - *Timers* → must be virtual in simulation anyway; provide `env.clock().sleep(d)`.
    A raw `tokio::time::sleep` in a protocol is a *bug* in simulation, not a lost feature.
  - *`tokio::spawn` / async mutexes* → hostile to determinism; if needed, expose a
    deterministic `spawn` on `Environment`.

The things a protocol "loses" (real sleeps, free task spawning, real-time locks) are
exactly what a deterministic simulator must forbid. So it's a guardrail, not a cage.

---

## 4. The step roadmap

| Step | What we build | Replaces / enables | Status |
|---|---|---|---|
| 1 | Hand-written single-task executor (`block_on`) | Teaches the `poll`/`Waker` machinery | ✅ done (then removed — scaffolding) |
| 2 | Multi-task deterministic scheduler (`run_tasks`/`run_with_idle`) | The simulator's main loop | ✅ done |
| 3 | Suspension primitive (`Recv` + `Switchboard`) | Explicit "blocked on recv"; deletes `yield_now` spin | ✅ done |
| 4 | Virtual-clock event loop | Deletes `has_data` guessing; deterministic timing | ✅ done |
| 5 | `Delay` over `ChannelConfig`; `SimNetwork: Network`; `simulate` driver | Real protocols run on the core | ✅ done |
| 5b | Trace + hook parity (`SimulationTrace`, `TriggeredHook`); suite migration; **legacy simulator deleted** | New core is canonical; old `simulator`/`context`/`manager`/`hook` modules and `Transport`/`SimulatedNetwork`/`SimulatedChannel` removed | ✅ done |
| 6 | Real async `TcpChannel` (`tokio-rustls`) | Same protocol over real TLS | ⏳ planned |
| 7 | Typed sub-protocol composition | Call-and-return nesting (the original goal) | ⏳ planned |

Steps 1–5b were built **additively** (old and new simulators coexisting); once the new core reached
output / trace / hook parity and the suite was migrated, the legacy tokio simulator was **deleted**.
The new core is now the only simulator.

Code lives in layers, with dependency arrows pointing one way
(`runtime` → {`executor`, `switchboard`, `network`}; `executor` and `switchboard` are independent):
- `src/net/simulation/executor.rs` — the scheduler, fully **network-agnostic** (`run_with_idle`,
  `Idle`, `TaskWaker`). No dependency on `Switchboard`.
- `src/net/simulation/switchboard.rs` — the network model (`Switchboard`, `Recv`, `Link`, `Delay`,
  `ConstantDelay`, `ConfigDelay`), plus per-party trace recording (`record_event`) and the hook
  extension point (`TriggeredHook`).
- `src/net/simulation/network.rs` — `SimNetwork`, the (now only) simulated `Network` impl.
- `src/net/simulation/runtime.rs` — `simulate` (the top-level driver: one `SimNetwork` per party,
  each protocol wrapped in a task; returns `SimulationOutcome { outputs, traces }`), the recording
  `drive` loop, and the private `run_simulation` executor↔switchboard idle-loop bridge.
- `src/net/simulation/channel.rs` — shared `ChannelConfig` / `NetworkConfig` / `ChannelId` and the
  timing math; `ChannelConfig::message_delay` added.

> **Layering note.** Dependencies point one way and the executor has **no** network dependency
> (`run_simulation` lives in `runtime.rs`). The legacy modules (`context`, `hook`, `manager`,
> `simulator`) and the legacy `Transport` / `SimulatedNetwork` / `SimulatedChannel` were **deleted**.
> `runtime.rs` is the canonical entry point — kept under that name (not renamed to `simulator.rs`).
> The `Endpoint` and `run_tasks` scaffolding from Steps 2–3 were removed when `SimNetwork`/`simulate`
> superseded them; the delay-model trait is named `Delay`.

---

## 5. Step 1 — Drive one future by hand (`block_on`)

**Goal:** poll a single `async` task to completion with no runtime, to learn the four
primitives the whole design rests on.

- **`Future`** — `fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Output>`.
- **`Poll::Ready/Pending`** — done, or parked. *In our simulator `Pending` will mean
  "this party is blocked on a recv."*
- **`Pin`** — an `async` block compiles to a self-referential state machine that must not
  move while polled; `Box::pin`/`pin!` fix its address.
- **`Context`/`Waker`** — the `Waker` is the handle a parked future keeps to later say
  "poll me again." *In our simulator the event loop calls `wake()` after delivering a
  message.*

The kernel polled in a loop; on `Pending` it required a synchronous self-wake (a yield),
otherwise it declared deadlock (in a single-task world nothing external can ever wake it).

**Outcome:** `block_on` and its `FlagWaker` were **scaffolding** and were **removed** once
Step 2's `run_tasks` subsumed them (`run_tasks` with one task is the single-task case).
A test is scaffolding when its *subject* is removed — that is why these tests were deleted,
in contrast to later tests whose subjects (the `Switchboard`) persist.

---

## 6. Step 2 — The N-task deterministic scheduler

**Goal:** generalize to N party futures on one thread. *Task `i` = party `i`*; the `Vec`
index is the `PartyId`.

Two generalizations from Step 1:
- the single boolean flag becomes a shared **ready queue** of task ids (`VecDeque<usize>`);
- the single waker becomes **one `TaskWaker` per task**, each carrying the task's `id`, so
  `wake()` pushes the *correct* id onto the queue.

```rust
struct TaskWaker { id: usize, ready_queue: Arc<Mutex<VecDeque<usize>>> }
impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) { self.wake_by_ref(); }
    fn wake_by_ref(self: &Arc<Self>) { self.ready_queue.lock().unwrap().push_back(self.id); }
}
```

The scheduler is a **dumb pump**: pop a ready id, poll it; on `Ready` mark done; on `Pending`
do nothing (the waker re-enqueues when progress is possible). When the ready queue empties
with tasks remaining, that is the **seam** later filled by the event loop.

**Why a ready queue, not "poll everyone each round":** with wakers, "ready queue empty"
*precisely means* "no task can make progress" — the exact signal Step 4 needs to decide
it is time to advance the virtual clock. Busy-polling would never produce that clean edge.

**Why `TaskWaker` carries an `id`:** a `Waker` is opaque and called from far away (later,
by the event loop). The shared ready queue must be told *which* task became runnable; the
`id` (= `PartyId` = `Vec` index) is the routing label welded into each waker at construction.

Final shape (after Step 4 added the idle hook):

```rust
pub enum Idle { Progressed, Deadlocked }

pub fn run_with_idle<F: FnMut() -> Idle>(tasks: Vec<Pin<Box<dyn Future<Output=()>>>>, mut on_idle: F) {
    // setup: tasks Vec<Option<…>>, ready_queue, one Waker per task, seed all ids
    while remaining > 0 {
        match ready_queue.lock().unwrap().pop_front() {
            Some(id) => { /* poll tasks[id]; Ready => drop+decrement; Pending => {} */ }
            None => match on_idle() {
                Idle::Progressed => {}                 // a delivery re-enqueued a task
                Idle::Deadlocked => panic!("deadlock"),
            },
        }
    }
}
pub fn run_tasks(tasks: Vec<Pin<Box<dyn Future<Output=()>>>>) {
    run_with_idle(tasks, || Idle::Deadlocked)
}
```

The `Poll::Pending => {}` and `Idle::Progressed => {}` arms are **structurally empty**: all
"what runs next" is encoded as ready-queue mutations performed by wakers; the loop never
decides it. They stay empty through Steps 5–7 (future logic lives in tasks and in the idle
handler, never in the pump). They would only gain code for optional instrumentation or a
different scheduling policy (fairness, budgets, cancellation) — none on the roadmap.

---

## 7. Step 3 — The suspension primitive (`Recv` + `Switchboard`)

**Goal:** make `Poll::Pending` *mean* "blocked on a recv." A blocked receive becomes an
explicit **park** (stash the waker, return `Pending`) instead of a spin. This is what
replaces the `recv` `yield_now` loop and the `has_data` guessing in `channel.rs`.

**Scope decision — instant delivery in Step 3.** A `send` delivers immediately
(enqueue + wake), giving a working zero-latency network that runs real two-party protocols
on the Step 2 scheduler. Step 4 inserts virtual delay; the `Recv` future and `Endpoint`
API do not change.

The router (`Switchboard`) holds, per directed **`Link { recipient, sender }`**, a FIFO of
delivered packets and at most one parked `Waker`. Keying by `(recipient, sender)` means a
`recv` by *i* from *j* and a `send` from *j* to *i* compute the *same* `Link` — **no
endpoint flipping** (unlike the old `Transport::flip_end_points`).

The heart — poll, then hand back or park:

```rust
fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Packet> {
    let mut sb = self.switchboard.lock().unwrap();
    match sb.try_recv(self.link) {
        Some(packet) => Poll::Ready(packet),
        None => { sb.park(self.link, cx.waker().clone()); Poll::Pending }  // register the intent
    }
}
```

`cx.waker()` is this party's `TaskWaker`, so the stashed clone routes a later `wake()` back
to the right party. The `ping_pong` test exercises both paths: a `recv` that finds its
message already queued (no park), and a `recv` that parks and is later woken by a `send`.

---

## 8. Step 4 — The virtual-clock event loop

**Goal:** move the wake from send-time to a **time-ordered event loop**. `send` now schedules
a delivery at `sender_clock + delay`; the scheduler, when *every* party is parked, pops the
**earliest** event, advances the receiver's virtual clock, delivers, and wakes. The `Recv`
future is untouched — protocols gain timing for free.

The `None` arm from Step 2 (ready queue empty, tasks remain) becomes "deliver the next
event"; it is a true deadlock only when the event queue is *also* empty.

`Switchboard` gains: an event heap, per-party clocks, a pluggable delay model, and a `seq`.

```rust
pub fn send(&mut self, from: PartyId, to: PartyId, packet: Packet) {
    let link = Link { recipient: to, sender: from };
    let arrival = self.clock_of(from) + self.delay.delay(link, packet.size());
    let seq = self.next_seq();
    self.events.push(Reverse(DeliveryEvent { arrival, seq, link, packet }));   // no immediate wake
}

pub fn deliver_next(&mut self) -> Idle {
    match self.events.pop() {
        Some(Reverse(ev)) => {
            let c = self.clock.entry(ev.link.recipient).or_default();
            *c = (*c).max(ev.arrival);                       // receiver waited until arrival
            self.queues.entry(ev.link).or_default().push_back(ev.packet);
            if let Some(w) = self.parked.remove(&ev.link) { w.wake(); }
            Idle::Progressed
        }
        None => Idle::Deadlocked,
    }
}
```

**Timing model (deliberate, documented simplifications):**
- *Modeled:* per-link latency applied at send; a receiver's clock jumps forward to a
  message's arrival (`max`), capturing "blocked waiting"; causal ordering (a reply cannot
  predate the request that unblocks it).
- *Simplified:* the **sender's clock does not advance on send** (real TCP has serialization
  cost); **local compute time is ignored** (a party's clock only moves on message receipt).
  The old `SimulationContext` instead measured *wall-clock* compute, which is richer but
  nondeterministic. Choosing "measured wall-clock" vs "modeled cost" is a real future
  decision, orthogonal to the event loop.

**The `seq` tiebreaker is a correctness requirement, not a nicety.** `BinaryHeap` does not
define pop order among equal keys, so two events at the same `arrival` could deliver in
run-to-run-unstable order, breaking determinism. `Ord` is `(arrival, seq)`.

**The `parked.remove == None` case (delivery with no waiter) returns `Idle::Progressed`.**
The idle hook only fires when all tasks are parked, so the recipient is either parked on a
*different* link (a message arrived ahead of what it currently awaits — it will `try_recv`
later) or has already finished. Both are normal, not deadlock. Returning `Progressed` is
**bounded**: during a run of consecutive idle deliveries no task is polled, so no new events
are scheduled; the heap strictly shrinks until a wake happens or `deliver_next` returns
`Deadlocked`. So real deadlocks are still caught, just after draining orphan events.

With a constant delay `D`, a ping→pong round trip yields party-1 clock `D` and party-0 clock
`2D`, derived deterministically with no `yield_now`, no `has_data`, no real sleeping.

---

## 9. Step 5 — Running a real `Protocol` on the core

**Goal:** a real `Protocol<N: Network>`, written exactly as for deployment, runs on the
Steps 1–4 engine with delays from the real `ChannelConfig`.

### The decisive design fork — the `Send` model

`#[async_trait]` makes every `Network::send_to/recv_from` return a **`Send`** future, but the
core is single-threaded on `Rc<RefCell<Switchboard>>` (`!Send`). Two resolutions:

- **Option X (chosen)** — make the core `Send` by switching to `Arc<Mutex<Switchboard>>`. The
  sim network then satisfies the **unchanged** `#[async_trait]` trait; `TcpNetwork`, the old
  simulator, everything else is untouched; new and old simulators **coexist**; deletion is a
  later cleanup. The `Mutex` never contends (one thread) — pure type-system bookkeeping.
- **Option Y** — relax the trait to `#[async_trait(?Send)]`, keep `Rc/RefCell` (cleaner, no
  locks), but re-annotate every impl and **replace the old `simulate()` now** (it uses a
  multi-thread `tokio::JoinSet`, which requires `Send`). Bigger, breaking cutover.

**Decision:** Option X, to keep the step additive and non-breaking. For the `Send` bound to
hold, `DelayModel: Send` (so `Box<dyn DelayModel>` and thus `Switchboard` are `Send`).

### The three pieces

1. **`DelayModel` over `ChannelConfig`.** Added `ChannelConfig::message_delay(n_bytes)`
   (`Tcp => recv_time_tcp(n)`, `Instant => ZERO`). `ConfigDelay<N: NetworkConfig>` maps a
   `Link` to `ChannelId::new(recipient, sender)` and reads the per-channel delay.
2. **`SimNetwork: Network`** — holds `{ local, parties, Arc<Mutex<Switchboard>> }`.
   `send_to` schedules via `switchboard.send`; `recv_from` awaits a `Recv`; `other`/`local_party`
   as today. No `MutexGuard` is held across an `.await`, so it is `Send` and deadlock-free.
3. **`simulate(config, protocols, hooks) -> SimulationOutcome { outputs, traces }`** (in `runtime.rs`)
   — `protocols: Vec<(PartyId, Box<dyn Protocol>)>` (paired, so a length mismatch is unrepresentable;
   distinct ids), `hooks: Vec<Arc<dyn TriggeredHook>>`. Derives the party set from the pair keys,
   builds one `SimNetwork` per party, runs each protocol through the recording `drive` loop, and
   returns per-party outputs **and** event traces keyed by `PartyId`. *(Evolved from an early
   `parties.zip(protocols)` form — which silently truncated — through a `HashMap` outputs-only return,
   to the current `SimulationOutcome` once trace + hook parity landed; see Step 5b below.)*

**Milestone test:** `real_protocol_runs_on_deterministic_core` runs a `SendRecv` protocol
(written *only* against `Network`) on two parties via `SimpleNetworkConfig`; party 0 receives
party 1's id and vice versa. This proves the Option-C thesis end to end.

### Step 5b — Trace + hook parity, suite migration, and deletion

The two follow-ons deferred from Step 5 were resolved:

- **Traces / events / hooks — done.** Per-party `SimulationTrace` is recorded by
  `Switchboard::record_event`: `SendData`/`ReceiveData` in `send`/`try_recv`, and
  `Start`/`ProtocolBegin`/`ProtocolEnd`/`Output`/`Stop` in `runtime::drive`. Hooks were **ported** to
  a new non-generic `TriggeredHook { trigger() -> Option<EventType>; run(party, &Event, &mut
  Switchboard) }`, fired from `record_event` and registered via `simulate`'s `hooks` argument. The old
  `Manager` is fully subsumed: outputs replace `handle_protocol_output`, traces replace
  `handle_simulator_output` (see D13).
- **Virtual clock in `Environment` — still deferred.** `env.clock()` is still wall-clock; the virtual
  clock lives in the `Switchboard` (`clock_of`). Wiring it into `Environment` is open follow-on work.

With parity reached, the six-scenario suite (`tests/simulator/simulator.rs`: SendRecv, PingPong,
Chained, BulkTransfer, OneWay, Broadcast) was **migrated** onto `runtime::simulate` + `SimulationOutcome`.
Protocols became `Protocol<SimNetwork>` (the config generic is gone — config is an argument to
`simulate`, not baked into the protocol type; D15); the `Manager`s were deleted; the `#[tokio::test]`
async tests became plain sync `#[test]`s asserting on `outcome.outputs` / `outcome.traces`. Two
behavioral deltas were accepted (D14): `CloseChannel` events disappear (no persistent channels in the
event-loop model, like `HasData`), and `Output` is recorded *after* `ProtocolEnd`.

All ten tests pass — the parity proof — and the legacy path was then **deleted**: `simulator.rs`,
`context.rs` (`SimulationContext`), `manager.rs`, `hook.rs`, plus the legacy `Transport` /
`SimulatedNetwork` (`network.rs`) and `SimulatedChannel` / `has_data` (`channel.rs`). `channel.rs`
keeps the shared `ChannelConfig` / `NetworkConfig` / `ChannelId` / timing math. The new core is the
only simulator.

**Known issue (open).** The trace `channel_id` in `send`/`try_recv` uses canonical
`Link::channel_id()` (min→max) instead of perspective-relative `ChannelId::new(recorder, peer)`, so
`Event::Display` arrows render backwards for the higher-id party. The migrated tests don't catch it
(they assert only `event_types`). See D10 / §15.

---

## 10. Step 6 — Real async TCP/TLS (planned)

**Goal:** the same protocols run over real sockets. The current `TcpChannel`
(`src/net/channel.rs`) is `async fn` on the surface but does **blocking** `write_all`/`read_exact`
on a blocking `StreamOwned`. That works today only because each party is a sequential task; the
moment a protocol awaits several peers concurrently (any broadcast round), blocking reads will
serialize or deadlock.

Plan:
- Replace the blocking rustls `StreamOwned` with **`tokio-rustls`** over a non-blocking
  `TcpStream`, so `send`/`recv` are genuinely async.
- Confirm the unchanged `SendRecv` (and richer protocols) run over real TLS, driven by tokio.
- Because Option X kept the `Network` trait `#[async_trait]` (Send), `TcpNetwork` can run on a
  multi-thread runtime; a single party also runs fine on a current-thread runtime.

Acceptance: a protocol binary with two processes completes a real TLS exchange, and the *same*
protocol type passes the simulator test.

---

## 11. Step 7 — Typed sub-protocol composition (planned)

**Goal:** the original requirement — *call a protocol within a protocol with a typed return.*
The trampoline (`ProtocolResult.next_protocol`) stays for **phase sequencing** and "emit bytes";
nesting gets its own abstraction:

```rust
#[async_trait(?Send)] // or Send, matching the chosen Network bound
trait SubProtocol<N: Network> {
    type Output;
    async fn run(self, env: &mut Environment<N>) -> Result<Self::Output, Error>;
}

// Composition is just .await with real data flow:
let triple  = BeaverTriple::new(...).run(env).await?;
let opened  = OpenShare::new(x - triple.a).run(env).await?;
```

On the deterministic executor this is "free": a sub-protocol is an `async` call that suspends
through the same `Network`/`Clock` points, so the simulator tracks nested protocols like any
other. Design tasks: how nested protocols appear in the trace (begin/end nesting), and whether
`SubProtocol` and `Protocol` share a supertrait.

---

## 12. Key concepts reference

**`Future` / `Poll` / `Pin` / `Waker`.** Polling = "try to make progress." `Pending` =
"parked; I kept your `Waker` and will call it when I can progress." `Pin` guarantees a
self-referential async state machine never moves while polled (`Box::pin` = stable heap
address; `as_mut().poll()` re-borrows without consuming). `Waker` is the one-way bridge from
"whatever makes the future ready" to "the executor that must re-poll it." Executor correctness
rule: **re-poll iff the waker fired.**

**Why wakes must be executor-controlled (determinism).** In the single-task kernel the only
possible wake is a *synchronous self-wake* during `poll`. In the full system there are exactly
two *controlled* sources: a self-wake (yield) and the **event loop** (after delivering a
message). We forbid *uncontrolled* wakes (a background thread the protocol spawned, a real OS
timer, real epoll readiness) because they are tied to wall-clock and the OS scheduler — they
destroy reproducibility, break virtual time, and ruin the "parked ⇒ simulator knows why"
invariant. On the *real* path, external wakes (epoll) are exactly right; the same protocol code
runs because the protocol only suspends through `Network`/`Clock` and never starts background work.

**`dyn` vs generic for `DelayModel`.** `Box<dyn DelayModel>` (not `D: DelayModel`) because a
generic would propagate virally — `Recv<D>`, `Endpoint<D>`, `run_simulation<D>`,
`SimNetwork<D>`, and into `Protocol` bounds — for an operation called once per send and dwarfed
by heap/HashMap/serialization cost. `dyn` also allows choosing the model at runtime
(config-driven, benchmark sweeps). The constructor takes `impl DelayModel + 'static` and boxes
internally for caller ergonomics. Reversible if profiling ever (implausibly) demanded it.

**The empty scheduler arms.** `Poll::Pending => {}` and `Idle::Progressed => {}` are empty by
design: this is a waker-driven pump, so routing lives in ready-queue mutations, not loop
branches. Wanting to put network logic there would signal a layering leak.

---

## 13. Decisions log

| # | Decision | Rationale |
|---|---|---|
| D1 | Option C over A/B | Sans-IO semantics (explicit blocking state) with async ergonomics; A doesn't fix the sim, B taxes protocol authoring |
| D2 | C1 (own executor) over C2 (`madsim`) | Precise control of scheduling/virtual time; no heavy dependency |
| D3 | Remove `block_on`/`FlagWaker` after Step 2 | Scaffolding subsumed by `run_tasks`; avoid two waker types / dead code |
| D4 | Key the router by `(recipient, sender)` | A `recv` and the matching `send` compute the same `Link`; no endpoint flipping |
| D5 | `seq` tiebreaker in the event heap | Deterministic pop order among equal arrival times |
| D6 | `parked.remove == None` ⇒ `Progressed` | Out-of-order arrival / finished recipient is normal; bounded by a shrinking heap; real deadlock still caught |
| D7 | `Box<dyn DelayModel>` over generic `D` | Avoid viral generics for a cold operation; runtime selectability |
| D8 | **Option X: `Arc<Mutex>` core** over `?Send` trait | Keep Step 5 additive/non-breaking; old and new simulators coexist; uncontended locks |
| D9 | Keep the `ProtocolResult` trampoline | It already models "emit bytes or run next"; add nesting separately (Step 7) |
| D10 | Unify on directed `Link`; retire `ChannelId`/`flip_end_points` | `Link {recipient,sender}` and `ChannelId {local,remote}` duplicate "ordered pair of parties"; `ChannelId`'s *perspective-relative* keying (hence `flip_end_points`) conflates perspective with identity and invites a latent orientation bug in `ConfigDelay` (only harmless while configs are symmetric). **Now (non-breaking):** centralize + canonicalize the bridge via `Link::channel_id()` and use it in `ConfigDelay`, so send/recv never disagree even under a direction-sensitive config. **Target (during old-simulator deletion):** re-key `NetworkConfig::channel_config` to directed `Link` (supports asymmetric links), update `SimpleNetworkConfig`, and delete `ChannelId`/`flip_end_points`. Not done now to preserve additivity (the old simulator still uses `ChannelId`). |
| D11 | Module layout: `executor` / `switchboard` / `network` / `runtime` | `simulate` is orchestration → `runtime.rs`; the network model → `switchboard.rs`; the new `Network` impl (`SimNetwork`) → `network.rs` beside the legacy one; the scheduler → `executor.rs`. Deps point one way: `runtime` → {`executor`, `switchboard`, `network`}. `run_simulation` lives in `runtime.rs` (private), so `executor.rs` has **no** network dependency. Dead Step 2–3 scaffolding (`run_tasks`, `Endpoint`) removed. `runtime.rs` becomes the canonical `simulator.rs` at cleanup. |
| D12 | Sequence cleanup (parity → migrate tests → delete), don't delete prematurely | The old simulator backed a live suite using `Manager`/hooks/`SimulationTrace`. Commit to Option C as a *direction*; delete only after trace/event parity and test migration. **Executed:** parity reached, suite migrated, legacy path deleted (D13–D15). |
| D13 | Port hooks as a non-generic `TriggeredHook` on the `Switchboard` | The new core isn't generic over `NetworkConfig`, so the trait drops the `<N>` param; `run(party, &Event, &mut Switchboard)` gives curated access — external hooks see only `Switchboard`'s pub API (`send`, `clock_of`), so they can't corrupt the event queue or recurse into `record_event`. Fired from `record_event`; registered via `simulate`'s `hooks` arg. `Manager` is subsumed by `SimulationOutcome { outputs, traces }`. |
| D14 | Drop `CloseChannel` and `HasData`; record `Output` after `ProtocolEnd` | Both events are artifacts of the old per-connection / polling design — the event-loop model has no persistent channels to close and no `has_data` polling. Output-after-End is just the order `drive` produces; both orderings are arbitrary, kept for simplicity. |
| D15 | Protocols are `Protocol<SimNetwork>`, not `Protocol<SimulatedNetwork<N>>` | `SimNetwork` isn't generic over the config; the config is an argument to `simulate`, not baked into the protocol type. One protocol bound works across all network configs (simplification surfaced by the migration). |

---

## 14. Design invariants to preserve

1. **Protocols suspend only through injected abstractions** (`Network`, and a virtual `Clock`).
   Never `tokio::time`, `tokio::spawn`, real sleeps, or background threads inside a protocol.
   This is what lets one protocol run on both the simulator and real TCP/TLS.
2. **All wakes originate from the executor** (self-wake or event loop). No uncontrolled wakes.
3. **The scheduler is a dumb pump.** Scheduling decisions are ready-queue mutations by wakers;
   the loop's `Pending`/`Progressed` arms stay empty. Network/protocol logic never leaks in.
4. **Determinism end to end.** Event ordering is total (`(arrival, seq)`); no reliance on
   `HashMap`/`BinaryHeap` iteration order or wall-clock for control flow.
5. **Additive until proven.** Build beside the old simulator; delete only after the new path
   covers the need. Deletion is the only irreversible step.
6. **`Poll::Pending` means exactly "blocked on the network"** for a party task — the property
   the whole virtual-time loop relies on.

---

## 15. Future strategies and open questions

**Concurrency in protocols.** Bless and document the `futures` combinators (`join!`, `select!`,
`try_join_all`, `FuturesUnordered`) for broadcast/round patterns. They are executor-agnostic and
need no tokio. Add a broadcast example/test that fans out and collects from all peers.

**Compute-time modeling.** Decide between (a) measured wall-clock compute (richer, nondeterministic
— what the old `SimulationContext` did) and (b) a modeled compute cost added to a party's clock
(deterministic). Likely (b), possibly with an injectable cost model paralleling `DelayModel`.

**Sender-side send cost.** Optionally advance the sender's clock by serialization/bandwidth time
(the old `adjust_send_time` added `recv_time_tcp` to the send time). Fold into the timing model.

**Traces and hooks — done (Step 5b).** `Event`/`SimulationTrace` recording and a ported
`TriggeredHook` now live on the new core. Open: how nested sub-protocols (Step 7) appear in the trace
(begin/end nesting), and the trace `channel_id` perspective bug (record from the recorder's view with
`ChannelId::new(recorder, peer)` rather than canonical `Link::channel_id()`; see D10 / Step 5b).

**Virtual `Clock` in `Environment`.** Replace wall-clock `Clock` with a handle over
`Switchboard::clock_of` so `env.clock()` reflects virtual time; protocols can then make
time-dependent decisions reproducibly.

**Adversarial / reordering harness.** A major payoff of explicit blocking state: a test runner
that delays, drops, or reorders deliveries (within the model) to fuzz protocol robustness — much
harder under the old tokio simulator.

**Packet loss and retransmission.** The model has a `PackageLoss` parameter; decide whether loss
is reflected only in throughput (current `lossy_throughput`) or also as dropped/retried deliveries
in the event loop.

**Unify `Link` and `ChannelId` (see D10).** Two types encode "an ordered pair of parties":
directed `Link {recipient, sender}` (the Switchboard's routing key, no flip) and perspective-relative
`ChannelId {local, remote}` + `flip_end_points` (legacy, old simulator + `NetworkConfig`). The
perspective-relative key is the wart and invites a latent orientation bug where `ConfigDelay` looks
up config from a fixed orientation that is only harmless because `ChannelConfig` is symmetric. *Now,
non-breaking:* add a single canonicalizing bridge `Link::channel_id()` and route `ConfigDelay`
through it. *Target, during the old-simulator deletion:* re-key `NetworkConfig::channel_config` to
directed `Link` (which also enables **asymmetric links** — different up/down bandwidth), then delete
`ChannelId`/`flip_end_points`. End state: one directed pair type for routing *and* config.

**Determinism guardrails.** Consider a lint/CI check or a `#[deny]`-style convention forbidding
tokio primitives in the protocol layer, to keep invariant #1 from eroding.

**Performance.** The single-thread executor and `Arc<Mutex>` are fine for simulation scale; if
large party counts or long protocols become slow, profile the event heap and per-poll locking
before considering structural changes.

---

*End of design record. Update the Decisions log and roadmap status as Steps 6–7 and the
follow-ons land.*
