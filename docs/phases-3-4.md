# Concurrent composition: how to complete Phases 3–5

This file keeps the name it was asked for, `phases-3-4.md`; it covers Phases 3, 4 and 5 of
`docs/concurrent-composition-plan.md`.

Written by Claude (AI) on 2026-11-09, on request, against `34c2856` plus the uncommitted follow-up
fixes of that day. It is a guide for implementing the three phases by hand: for each phase, the
decisions left to you, the skeleton, the tests, the code, and what changes for users.

Everything here is a proposal. Each claim says where to re-derive it from. Where the guide departs
from the plan it says *(departs from the plan)*, and where it adds something the plan does not have
it says *(addition)*.

**Rationale.** Wherever a mechanism is not the obvious one, a paragraph or a bullet headed **Why**
says what it is for and what the alternative was. Three kinds of support are kept apart:

- a library's documented behaviour, cited as `crate version, path:line`. These were read in the
  pinned sources under `~/.cargo/registry/src/index.crates.io-*/`, and for the standard library
  under `$(rustc --print sysroot)/lib/rustlib/src/rust/library/` (rustc 1.99.1);
- the plan, or the code of this repository, cited as everywhere else;
- my own reasoning, marked *(AI reasoning)*, with the place to re-derive it from.

**Contents**

0. What was checked, and what was not
1. Before Phase 3: the TCP tests' port helper
2. Phase 3 — `TcpNetwork` sessions
3. Phase 4 — session-aware tracing
4. Phase 5 — link occupancy
5. Order of work

---

## 0. What was checked, and what was not

All three phases were prototyped in a scratch copy of the repository, outside it. The `rust` code
blocks below were copied from that prototype by a script, after `cargo fmt`, so they are the code
that was compiled and run. The one exception is the skeleton in §2.3, which says so. The prototype
itself was in a temporary directory and is not kept.

Final run, from a clean build, with all three phases in place:

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` | clean |
| `cargo test --all-features --no-fail-fast` | 280 passed, 3 failed, 5 ignored |

The 3 failures are three golden scenarios whose timestamps Phase 5 shifts, which is the phase's
intended effect (§4.7). With occupancy switched off, so only Phases 3 and 4 are active, the same
command gave 280 passed and 2 failed, the two being the Phase 5 timing tests, and the golden
snapshot passed 11 of 11.

Per phase:

| Phase | Tests written | Before the implementation | After |
| --- | --- | --- | --- |
| 3 | 10 new, in `tcp.rs` | not run against a skeleton | 17 of 17 TCP tests in 100 consecutive runs (and in 150 before `biased;` was added to T3); also 16 of 16 in 100 runs with every test on a 4-worker multi-threaded runtime (before the tenth test was added) |
| 4 | 4, in `tests/trace_tree.rs` | 0 of 4 pass | 4 of 4; golden 11 of 11, byte-identical |
| 5 | 3, in `tests/occupancy.rs` | 1 of 3 pass | 3 of 3; golden 8 of 11 (§4.7) |

For Phase 3, five deliberate breaks were also applied one at a time, and each was caught (§2.4).

**Not checked:**

- Anything over a real network. Every TCP test ran on loopback.
- The `tc netem` side of the benchmark. It needs root, so only the simulated predictions were
  produced (§4.8). The Phase 5 acceptance criterion is therefore still open.
- More than 3 parties on TCP, frames near the codec's size limit, and what happens when the tokio
  runtime shuts down while writer tasks still hold queued frames.
- Memory use under a peer that sends to sessions nobody opens (§2.2, decision 3).
- The opt-in route for occupancy (§4.9), which is only sketched.

---

## 1. Before Phase 3: the TCP tests' port helper

The existing TCP tests are flaky on their own, before any change, and Phase 3 more than doubles
the number of tests that share the helper. Fix this first, or Phase 3's exit condition ("the
existing inline `tcp.rs` tests pass") cannot be judged.

**Evidence.** The repository's own test binary, with `tcp.rs` unchanged, run 300 times back to
back on the 7 TCP tests (`target/debug/deps/scl_rs-… net::tcp`): 94 runs failed. Every failure
was `IoError(Os { code: 98, kind: AddrInUse })`. With Phase 3's tests added and the same helper,
175 of 300 runs failed, again all `AddrInUse` (273 failing tests, none with another cause).
Under `cargo test`, which leaves about a second between runs, the failure is much rarer: it
showed once in about 25 runs.

**Cause** *(AI reading of `src/net/tcp.rs:459-465`)*. `free_base_port` asks the OS for one free
port and returns it. Party `i` then listens on `base + i`. Only the base was ever reserved, so
`base + 1` may belong to another test running in parallel, or be the source port of somebody's
connection.

**Fix.** Hand the ports out from a counter, in blocks, below the range the OS picks from. With
this helper the prototype's TCP tests failed 0 times in 300 runs, and 0 in 150 after the last test
was added:

```rust
/// Returns the base of a block of ports that no other test of this binary uses.
///
/// Party `i` listens on `base + i`, so a test needs a run of consecutive ports. Asking the OS
/// for one free port reserves only that one: its neighbours may belong to another test running
/// in parallel, or be the source port of some connection. The blocks are handed out from a
/// counter instead, below Linux's default ephemeral range (32768-60999), which is where the OS
/// picks source ports from.
fn free_base_port() -> u16 {
    const BLOCK: u16 = 8;
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(20_000);
    NEXT.fetch_add(BLOCK, std::sync::atomic::Ordering::Relaxed)
}
```

The signature is unchanged, so the existing tests need no edit. On this machine the ephemeral
range is 32768–60999 (`/proc/sys/net/ipv4/ip_local_port_range`). The remaining risk is another
process on the machine already listening on the ports from 20000 upwards that the tests take
(8 per test).

**Why a counter, and not a better search for free ports.**

- *The ports must be consecutive.* The configuration carries one `base_port`, and party `i`
  listens on `base_port + i` (`src/net/mod.rs:376`, `tcp.rs:182`). A test cannot give each party
  whatever port happens to be free.
- *Searching has the same gap as today.* Any scheme that finds free ports and then releases them
  leaves a moment, before `TcpNetwork::create` binds them, in which another test can take one.
  Probing all `n` ports makes the gap smaller; it does not close it.
- *A counter closes it by construction.* No two tests of the binary ever get overlapping blocks.
  A `static` is enough, because the tests that can collide are threads of one process. *(That
  cargo runs the test binaries themselves one after another is from memory, not checked.)*
- *Below the ephemeral range,* because that range is where the OS takes the source port of every
  outgoing connection, the tests' own client connections included.
- *Using the same ports again in the next run is safe.* A listener is bound with `SO_REUSEADDR`
  on Unix (mio 1.2.0, `src/net/tcp/listener.rs:63`). The 300 back-to-back runs are the evidence.
- *`BLOCK = 8`* leaves room for a test with up to 8 parties. The largest today has 3.

---

## 2. Phase 3 — `TcpNetwork` sessions

### 2.1 The shape

Today (`src/net/tcp.rs:86-99`) a `TcpNetwork` owns its sockets: a `PeerWriter` per peer
(`:39-78`) and a `StreamMap` of readers. After this phase it is a small handle over a shared core:

```
TcpNetwork { local, core: Arc<TcpCore>, session, next_child }      one per session, not Clone
        │
        ▼
TcpCore { parties, writers, mailboxes, readers }                   one per execution
   writers[peer]  ──WriterCmd──▶  writer task  ──▶  write half of the TLS stream
   mailboxes      ◀──packets───   reader task  ◀──  read half of the TLS stream
```

- A **send** encodes one complete frame and queues it for that peer's writer task. It never waits.
- A **reader task** decodes each frame and files the packet under `(session, sender)`.
- A **receive** looks only in its own session's mailbox, and registers its waker there if the
  mailbox is empty.

That is the plan's §5.3. The two tasks per peer are decision C-4; the writer task is what makes a
send safe to cancel (R-1).

**Why a handle over a shared core.** `ForkNetwork::fork(&mut self) -> Self` returns an owned
handle of the same type (`src/net/mod.rs:609`), and a `Join` then drives all the children at the
same time, each through `&mut` to its own handle (`src/protocol/concurrent.rs:150-151`). The
sockets therefore cannot belong to one handle. They move into a core that every handle shares
through an `Arc`, and a handle keeps only what differs per session: `session` and `next_child`.

**Why a reader task per peer, and not receives that read the socket themselves** (C-4).

1. *A mailbox is needed in any design.* One stream carries the frames of every session, in the
   order they were sent. A receive in session A that reads the socket will meet frames for
   session B, and has to leave them where B will find them. So the only open question is who does
   the reading.
2. *If the receives do it, wake-ups can be lost.* Tokio keeps one waker per stream and direction:
   "only the `Waker` from the `Context` passed to the most recent call is scheduled to receive a
   wakeup" (tokio 1.53.1, `src/net/tcp/stream.rs:330-333`). Two branches that poll the same stream
   with different wakers overwrite each other's registration, and the first is never woken.
   Branches of a `Join` share their task's waker, so they would get away with it. Branches of a
   `JoinAll` of more than 30 protocols would not: above that size `try_join_all` switches to
   `FuturesOrdered` (futures-util 0.3.34, `src/future/join_all.rs:35`,
   `src/future/try_join_all.rs:138`), whose futures "will only be polled when they generate
   wake-up notifications" (`src/stream/futures_unordered/mod.rs:51-52`), so each has a waker of
   its own. *(AI reasoning over those sources; the plan states the conclusion in §5.3. Not
   reproduced in a test.)*
3. *With a reader task, a receive is only a mailbox operation.* It has the shape of the
   simulator's receive (`src/net/simulation/switchboard/recv.rs:101-116`): look, take, or register
   and wait. That shape is what makes a receive safe to drop (§2.6).
4. *A peer's end reaches every session at once,* including the sessions that are not receiving at
   that moment.

The cost: nothing slows a fast sender down, because a reader drains its socket whether or not
anybody is receiving (plan §10).

**Why a writer task per peer, and not a lock around the write half** (R-1). `write_all` "is not
cancel safe": if its future is dropped, "the provided buffer may have been partially written"
(tokio 1.53.1, `src/io/util/async_write_ext.rs:355-361`). Fail-fast drops a sibling branch at
whatever `.await` it is suspended on. If that is a `write_all`, the first part of a frame is on
the stream and the rest never follows, and the next frame's bytes are read as the remainder of the
broken one. Every session on that link is then corrupted, not only the branch that was dropped. A
lock does not help: dropping the future releases the lock with the frame half written. With a
writer task, the future that can be dropped never touches the socket, and the task that touches
the socket is never dropped in the middle of a frame.

**Why the queue to the writer is unbounded.** `UnboundedSender::send` is not `async`, because
sending "never requires any form of waiting" (tokio 1.53.1, `src/sync/mpsc/unbounded.rs:536-539`).
`send_to` then never suspends, so there is no point at which it can be dropped half done. A
bounded channel would be correct too: its `send` guarantees that a dropped send "was not sent"
(`src/sync/mpsc/bounded.rs:776-781`). It would bound the queue, at the price of a `send_to` that
waits when the socket is slow. The plan takes the unbounded queue and records the missing
back-pressure as a risk (§10).

### 2.2 Decisions that are yours

Each has a compatibility or security consequence, so the guide does not settle it. The prototype's
choice is stated so that the code below can be read. Decision 1 has since been made (2026-11-10);
the others are still open.

1. **How one peer's failure reaches several sessions. Decided by Hernán, 2026-11-10: the
   `PeerFailed` variant.** `NetworkError` is not `Clone`, and a failure has to be reported to
   every session that later touches the peer. The prototype adds a variant *(addition)*; the enum
   is `#[non_exhaustive]` (`src/net/mod.rs:74`), so this breaks no `match`:

   ```rust
   /// The connection with a peer failed: a read, a decode or a write went wrong.
   #[error("the connection with peer {peer} failed: {reason}")]
   PeerFailed {
       /// The peer whose connection failed.
       peer: PartyId,
       /// What went wrong, as text.
       reason: String,
   },
   ```

   Alternative: keep the original error for the first receive that sees it, and give later ones
   `ConnectionClosed(Some(peer))`. **Why the variant:** every session then learns the same thing,
   whichever asks first.

   **The field type is still to be made consistent.** In the working tree (2026-11-10) the variant
   has `reason: &'static str` (`src/net/mod.rs:134`), while `PeerState::Failed` holds a `String`
   (`src/net/tcp.rs:99`), as in this guide's code. The two have to match:

   - With `String`, `peer_error` clones the stored text, and the underlying error's message is
     kept, for example rustls's `close_notify` text (§2.7).
   - With `&'static str`, `PeerState::Failed` holds a `&'static str` too, and the reader and
     writer tasks store a fixed text such as `"read failed"`. The message of the `io::Error` or
     `postcard::Error` is then dropped: a text built at run time, such as `error.to_string()`, is
     not `&'static` unless it is leaked (`String::leak`).
2. **`recv_any` while one peer has failed.** The plan says only that it returns
   `ConnectionClosed(None)` once every peer is closed and drained (§5.3). The prototype keeps
   waiting while any remote peer is open, and `recv_any` never reports a single peer's failure.
   Today a failed stream's error comes out of `recv_any` at once (`tcp.rs:274-279`). **Why wait:**
   `recv_any` is how a party collects from *any* `n − t` peers, and that use should survive one
   peer crashing.
3. **Unbounded buffering.** A reader files frames for whatever session path a peer names, including
   sessions never forked here, with paths as long as a frame allows. The plan lists bounding this
   as a non-goal (§2) and as a risk (§10). The frame size limit is the codec's default, 8 MiB
   (`tokio-util` 0.7.19, `src/codec/length_delimited.rs:694`).
4. **Sends to a peer that closed cleanly.** The prototype refuses a send only when the peer has
   *failed*. After a clean EOF on the read side, a send is still queued. **Why:** a clean EOF says
   only that the peer has finished *sending*; in TLS 1.3 a `close_notify` closes one direction
   (RFC 8446 §6.1, from memory, not re-read). If the socket is in fact gone, the write fails and
   the peer becomes `Failed`.
5. **Whether `recv_any` includes this party's own loop-back packets.** The prototype does, as
   today's `StreamMap` does. On the simulator the untimed `recv_any` leaves the local party out
   (`src/net/simulation/network.rs:72-84`) and the timed one includes it (`:86-95`). **Why keep
   it:** it is the choice that changes nothing for code that exists (G6).
6. **What the root handle's `close()` means for other sessions.** *(addition)* In the prototype it
   also marks every peer closed, so a receive still waiting in a forked session ends with
   `ConnectionClosed` (§2.8). Without that, such a receive never returns: the test for it hung
   before the change.
7. **The frame header.** The To-do bullet says `postcard((SessionId, Vec<Element>))`. The plan
   says the path only, with the root tag left off the wire (§5.1, §5.3), and `SessionId` has no
   serde derive. The prototype follows the plan.

### 2.3 Step 1 — Skeleton

Give `TcpNetwork` its `session` and `next_child` fields, and the `ForkNetwork` impl with `todo!()`
bodies, so that the tests of the next step compile:

```rust
impl ForkNetwork for TcpNetwork {
    fn fork(&mut self) -> Self { todo!() }
    fn session(&self) -> &SessionId { todo!() }
}
```

*(This skeleton was not compiled on its own: the prototype was written in full first.)*

### 2.4 Step 2 — Tests

They go in `tcp.rs`'s `mod tests`, because the certificate helpers (`write_party_certs`,
`write_config_files`, `load_config`) are private to it. The imports and aliases they need, besides
`use futures_util::future::try_join_all;`:

```rust
use crate::math::field::mersenne61::Mersenne61;
use crate::protocol::concurrent::Join;
use crate::protocol::passive_shamir::{
    double_rand_share::PassiveRandDoubleShr, rand_share::PassiveRandShr,
};
use crate::protocol::{
    Environment, Error, ForkEnvironment, GeneralEnv, Protocol, ProtocolId, RandEnvironment,
};
use crate::ss::shamir::{DoubleShare, ShamirSS};
use crate::ss::LinearShare;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

type F = Mersenne61;
type Share = ShamirSS<1, F>;
```

Three helpers. `connected` removes the setup that every test repeats. `within_10s` matters more
than it looks: **a routing bug makes these tests hang, not fail**, because the receive it breaks
never returns. With the guard, the same bug fails the test after 10 s (checked on T3 with the
first break of the table below).

```rust
/// `n` networks connected to each other over mTLS loopback, indexed by party.
async fn connected(n: usize) -> Vec<TcpNetwork> {
    let dir = tempfile::tempdir().unwrap();
    write_party_certs(&dir, n);
    write_config_files(&dir, n, free_base_port());
    try_join_all((0..n).map(|i| TcpNetwork::create(i, load_config(&dir, i), test_root_tag())))
        .await
        .unwrap()
}

/// Runs `test`, and fails instead of hanging if it does not finish: a routing bug shows up as
/// a receive that never returns.
async fn within_10s<T>(test: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), test)
        .await
        .expect("the test did not finish within 10 s")
}

/// A packet holding `tag`, so that packets can be told apart by equality.
fn packet(tag: u64) -> Packet {
    let mut packet = Packet::empty();
    packet.write(&tag).unwrap();
    packet
}

fn party(id: usize) -> PartyId {
    PartyId::from(id)
}
```

What each test is for, and the deliberate break of the prototype that it caught:

| Test | Break that it caught |
| --- | --- |
| T3 `t3_sessions_on_one_tls_link_receive_only_their_own_packets` | A: frames filed under the root session, the path ignored |
| T5 `t5_a_frame_for_a_session_not_yet_forked_waits_for_the_fork` | A |
| T8 (a) `t8_join_of_two_protocols_over_tls` | A |
| T8 (b) `t8_a_timed_out_receive_does_not_lose_the_next_packet` | A |
| T8 (c) `t8_a_join_that_fails_fast_leaves_the_stream_usable` | A |
| T8 (d) `t8_a_closing_peer_ends_the_receives_of_every_session` | B: a peer's end wakes nobody |
| T8 (e) `t8_dropping_the_network_without_close_closes_the_connections` | B; and C: the writer task does not shut down when the core is dropped |
| `after_close_receives_and_sends_end_with_connection_closed` *(addition)* | hung before decision 6 was implemented |
| `recv_any_takes_the_lowest_ready_sender` *(addition)* | D: `recv_any` taking the highest sender |
| `frames_for_a_retired_session_are_discarded` *(addition)* | E: retired sessions still queued |

Break B was also caught by the existing `recv_from_closed_peer_reports_connection_closed`. Breaks D
and E were caught by nothing until their two tests were added.

**T3.** The receive in session A has to be waiting *before* anything is sent, or the test does not
exercise the case it is named after. **Why `biased;`:** `tokio::join!` polls in the order written
only when told to. By default it "rotates which contained future is polled first whenever it is
woken" (tokio 1.53.1, `src/macros/join.rs:35-40`).

```rust
/// T3: on one link, session B's frame is written before session A's, while the receiver
/// already waits in A. Each session receives its own packet.
#[tokio::test]
async fn t3_sessions_on_one_tls_link_receive_only_their_own_packets() {
    let mut nets = connected(2).await;
    let (mut net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());
    let (mut a0, mut b0) = (net0.fork(), net0.fork());
    let (mut a1, mut b1) = (net1.fork(), net1.fork());

    within_10s(async {
        // `biased;` makes `join!` poll in the order written: the receive in A is pending
        // before anything is sent. Without it, `join!` rotates which future it polls first.
        let (received_a, ()) = tokio::join!(biased; a1.recv_from(party(0)), async {
            b0.send_to(party(1), &packet(2)).await.unwrap();
            a0.send_to(party(1), &packet(1)).await.unwrap();
        });
        assert_eq!(received_a.unwrap(), packet(1));
        assert_eq!(b1.recv_from(party(0)).await.unwrap(), packet(2));
    })
    .await;
}
```

In the prototype only this test and the one after `close()` wear the guard. Put it on all of
them.

**T5.** The barrier is the trick to reuse: one reader files a link's frames in order, so receiving
a *later* root-session packet proves the earlier frame was filed. **Why not a sleep:** a sleep is
slower, and still only probably long enough. The barrier is exact, and costs one packet.

```rust
/// T5: a frame for session 0.3 arrives before the receiver has forked that session. It waits,
/// and the receiver gets it once it forks down to 0.3.
#[tokio::test]
async fn t5_a_frame_for_a_session_not_yet_forked_waits_for_the_fork() {
    /// Forks `handle` four times and returns the handle on its child 3.
    fn child_3(handle: &mut TcpNetwork) -> TcpNetwork {
        for _ in 0..3 {
            handle.fork();
        }
        handle.fork()
    }

    let mut nets = connected(2).await;
    let (mut net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());

    let mut branch0 = net0.fork();
    let mut early0 = child_3(&mut branch0);
    assert_eq!(early0.session().to_string(), "0.3");
    early0.send_to(party(1), &packet(7)).await.unwrap();
    // One reader files the frames of a link in order, so once this root-session packet is
    // received, the frame for 0.3 has been filed too.
    net0.send_to(party(1), &packet(0)).await.unwrap();
    assert_eq!(net1.recv_from(party(0)).await.unwrap(), packet(0));

    let mut branch1 = net1.fork();
    let mut early1 = child_3(&mut branch1);
    assert_eq!(early1.recv_from(party(0)).await.unwrap(), packet(7));
}
```

**T8 (a).** Three parties, a `Join` of two protocols of different types. **Why the outputs are
opened, not compared share by share:** forking draws from the parent's RNG, so the shares
themselves depend on how the protocols were composed (`src/protocol/concurrent.rs:57-64`).

```rust
/// T8 (a): three parties run a `Join` of two protocols of different types over mTLS.
struct RandomAndDoubles;

const T8_PARTIES: usize = 3;
const T8_THRESHOLD: usize = 1;

/// The random sharings, and the two halves of each double sharing.
type RandomAndDoublesOutput = (Vec<Share>, Vec<(Share, Share)>);

impl<E: RandEnvironment + ForkEnvironment> Protocol<E> for RandomAndDoubles {
    type Output = RandomAndDoublesOutput;

    async fn run(self, env: &mut E) -> std::result::Result<Self::Output, Error> {
        let parties = env.network().party_ids();
        let (random, doubles) = Join((
            PassiveRandShr::<1, F>::new(T8_THRESHOLD, parties.clone())?,
            PassiveRandDoubleShr::<1, F>::new(T8_THRESHOLD, parties)?,
        ))
        .execute(env)
        .await?;
        let doubles = doubles.into_iter().map(DoubleShare::into_parts).collect();
        Ok((random, doubles))
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("RandomAndDoubles")
    }
}

#[tokio::test]
async fn t8_join_of_two_protocols_over_tls() {
    let parties: Vec<PartyId> = (0..T8_PARTIES).map(PartyId::from).collect();
    let runs =
        connected(T8_PARTIES)
            .await
            .into_iter()
            .enumerate()
            .map(|(i, network)| async move {
                let mut env = GeneralEnv::new(network, ChaCha20Rng::seed_from_u64(i as u64));
                let output = RandomAndDoubles.execute(&mut env).await?;
                env.network.close().await?;
                Ok::<_, Error>(output)
            });
    let outputs = try_join_all(runs).await.unwrap();

    let open = |get: &dyn Fn(&RandomAndDoublesOutput) -> Share| {
        let shares: Vec<Share> = outputs.iter().map(get).collect();
        <Share as LinearShare>::secret_from_shares(&shares, &parties).unwrap()
    };
    for k in 0..T8_PARTIES - T8_THRESHOLD {
        assert_ne!(open(&|o| o.0[k].clone()), F::from(0u64));
        assert_eq!(
            open(&|o| o.1[k].0.clone()),
            open(&|o| o.1[k].1.clone()),
            "double sharing {k}: the two halves hide different secrets"
        );
    }
}
```

**T8 (b).** Receive cancel-safety:

```rust
/// T8 (b): a receive that times out loses nothing. The packet that arrives afterwards is
/// received by the next receive in the same session.
#[tokio::test]
async fn t8_a_timed_out_receive_does_not_lose_the_next_packet() {
    let mut nets = connected(2).await;
    let (mut net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());
    let (mut a0, mut a1) = (net0.fork(), net1.fork());

    let timed_out = a1
        .recv_from_with_timeout(party(0), Duration::from_millis(50))
        .await;
    assert!(
        matches!(&timed_out, Err(NetworkError::Timeout(Some(peer))) if *peer == party(0)),
        "expected Timeout(Some(0)), got {timed_out:?}"
    );
    let timed_out = a1.recv_any_with_timeout(Duration::from_millis(50)).await;
    assert!(
        matches!(&timed_out, Err(NetworkError::Timeout(None))),
        "expected Timeout(None), got {timed_out:?}"
    );

    a0.send_to(party(1), &packet(9)).await.unwrap();
    assert_eq!(a1.recv_from(party(0)).await.unwrap(), packet(9));
}
```

**T8 (c).** Send cancel-safety. Note its limit: in this design a send has no await point at which
it could be dropped, so the test passes by construction. It checks that the queued frame still
arrives whole and that the parent session works afterwards. It would not reliably catch a return
to writing inside `send_to`. **Why `SendThenWait` comes first in the tuple:** `try_join!` polls
its futures in the order written (futures-macro 0.3.34, `src/join.rs:99-103`), so the frame is
queued before `Fail` fails.

```rust
/// Sends one large packet to the other party, then waits for a reply that never comes.
struct SendThenWait(Packet);

impl<E: Environment> Protocol<E> for SendThenWait {
    type Output = ();

    async fn run(self, env: &mut E) -> std::result::Result<(), Error> {
        let other = env.network().other()?;
        env.network_mut().send_to(other, &self.0).await?;
        env.network_mut().recv_from(other).await?;
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("SendThenWait")
    }
}

/// Fails at once.
struct Fail;

impl<E: Environment> Protocol<E> for Fail {
    type Output = ();

    async fn run(self, _env: &mut E) -> std::result::Result<(), Error> {
        Err(Error::Input)
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Fail")
    }
}

/// T8 (c): a `Join` fails fast while its other branch has a 1 MiB frame under way. The frame
/// still arrives whole, and the parent session works afterwards: the dropped branch left no
/// partial frame on the stream.
#[tokio::test]
async fn t8_a_join_that_fails_fast_leaves_the_stream_usable() {
    let mut nets = connected(2).await;
    let (mut net1, net0) = (nets.pop().unwrap(), nets.pop().unwrap());
    let mut big = Packet::empty();
    big.write(&vec![0xABu8; 1024 * 1024]).unwrap();

    let mut env0 = GeneralEnv::new(net0, ChaCha20Rng::seed_from_u64(0));
    // `SendThenWait` is polled first: it queues its frame and waits. `Fail` then fails, and
    // the `Join` drops `SendThenWait` where it is.
    let joined = Join((SendThenWait(big.clone()), Fail))
        .execute(&mut env0)
        .await;
    assert!(matches!(joined, Err(Error::Input)), "the Join must fail");
    env0.network.send_to(party(1), &packet(5)).await.unwrap();

    // Party 1 forks as its own `Join` would: the large frame is in child session 0.
    let (mut branch, _other_branch) = (net1.fork(), net1.fork());
    assert_eq!(branch.recv_from(party(0)).await.unwrap(), big);
    assert_eq!(net1.recv_from(party(0)).await.unwrap(), packet(5));
}
```

**T8 (d) and (e).** A peer's end reaches every session, with and without `close()`. In (d) the
three receives are of three kinds on purpose: a `recv_from` in a forked session, a `recv_any` in
another, and a `recv_from` in the root session.

```rust
/// T8 (d): when a peer closes, the receives waiting for it in every session end with
/// `ConnectionClosed`.
#[tokio::test]
async fn t8_a_closing_peer_ends_the_receives_of_every_session() {
    let mut nets = connected(2).await;
    let (mut net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());
    let (mut a1, mut b1) = (net1.fork(), net1.fork());

    let (in_a, in_b, in_root, closed) = tokio::join!(
        a1.recv_from(party(0)),
        b1.recv_any(),
        net1.recv_from(party(0)),
        net0.close(),
    );
    closed.unwrap();
    for result in [in_a, in_root] {
        assert!(
            matches!(&result, Err(NetworkError::ConnectionClosed(Some(peer))) if *peer == party(0)),
            "expected ConnectionClosed(Some(0)), got {result:?}"
        );
    }
    assert!(
        matches!(&in_b, Err(NetworkError::ConnectionClosed(None))),
        "expected ConnectionClosed(None), got {in_b:?}"
    );
}
```

```rust
/// T8 (e): dropping the last handle without `close()` still closes the connections, so the
/// peer's receive ends with `ConnectionClosed`.
#[tokio::test]
async fn t8_dropping_the_network_without_close_closes_the_connections() {
    let mut nets = connected(2).await;
    let (mut net1, net0) = (nets.pop().unwrap(), nets.pop().unwrap());
    drop(net0);

    let result = net1.recv_from(party(0)).await;
    assert!(
        matches!(&result, Err(NetworkError::ConnectionClosed(Some(peer))) if *peer == party(0)),
        "expected ConnectionClosed(Some(0)), got {result:?}"
    );
}
```

**After `close()`** *(addition, decision 6)*:

```rust
/// After the root handle's `close()`, nothing more can arrive. A receive, in the root session or
/// in a forked one, ends with `ConnectionClosed` instead of waiting for ever, and a send fails.
#[tokio::test]
async fn after_close_receives_and_sends_end_with_connection_closed() {
    let mut nets = connected(2).await;
    let (_net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());
    let mut forked = net0.fork();
    net0.close().await.unwrap();

    within_10s(async {
        for result in [
            net0.recv_from(party(1)).await,
            forked.recv_from(party(1)).await,
            net0.send_to(party(1), &packet(1)).await.map(|_| packet(0)),
        ] {
            assert!(
                matches!(&result, Err(NetworkError::ConnectionClosed(Some(peer))) if *peer == party(1)),
                "expected ConnectionClosed(Some(1)), got {result:?}"
            );
        }
    })
    .await;
}
```

**`recv_any` order** *(addition)*. This is the plan's G6 change, and nothing else tests it:

```rust
/// `recv_any` takes from the lowest ready sender, as the simulator does. Parties 1 and 2 each
/// send one packet in a child session, then one in the root session. Once party 0 has received
/// both root packets, both child packets are filed, whatever order they arrived in.
#[tokio::test]
async fn recv_any_takes_the_lowest_ready_sender() {
    let mut nets = connected(3).await;
    let (mut net2, mut net1, mut net0) = (
        nets.pop().unwrap(),
        nets.pop().unwrap(),
        nets.pop().unwrap(),
    );
    let (mut a0, mut a1, mut a2) = (net0.fork(), net1.fork(), net2.fork());

    // Party 2 sends first.
    for (child, root, me) in [(&mut a2, &mut net2, 2), (&mut a1, &mut net1, 1)] {
        child.send_to(party(0), &packet(me)).await.unwrap();
        root.send_to(party(0), &packet(0)).await.unwrap();
    }
    net0.recv_from(party(2)).await.unwrap();
    net0.recv_from(party(1)).await.unwrap();

    assert_eq!(a0.recv_any().await.unwrap(), (party(1), packet(1)));
    assert_eq!(a0.recv_any().await.unwrap(), (party(2), packet(2)));
}
```

**Retired sessions** *(addition)*. The effect is only visible in the mailbox map, which an
in-crate test can read:

```rust
/// Dropping a forked handle retires its session: what was queued for it is freed, and a frame
/// that arrives for it later is discarded instead of queued.
#[tokio::test]
async fn frames_for_a_retired_session_are_discarded() {
    let mut nets = connected(2).await;
    let (mut net1, mut net0) = (nets.pop().unwrap(), nets.pop().unwrap());
    let mut a0 = net0.fork();
    let a1 = net1.fork();
    let session = a1.session().clone();

    // One frame arrives before the handle is dropped, one after.
    a0.send_to(party(1), &packet(1)).await.unwrap();
    net0.send_to(party(1), &packet(0)).await.unwrap();
    net1.recv_from(party(0)).await.unwrap();
    assert!(lock(&net1.core.mailboxes).sessions.contains_key(&session));
    drop(a1);
    assert!(!lock(&net1.core.mailboxes).sessions.contains_key(&session));

    a0.send_to(party(1), &packet(2)).await.unwrap();
    net0.send_to(party(1), &packet(0)).await.unwrap();
    net1.recv_from(party(0)).await.unwrap();
    assert!(!lock(&net1.core.mailboxes).sessions.contains_key(&session));
}
```

### 2.5 Step 3 — Frame format

The frame body becomes `postcard((path, elements))`. The root tag is not sent; the reader rebuilds
the session from its own tag (plan §5.3).

```rust
/// Bytes of the little-endian length prefix of a frame.
const LENGTH_PREFIX_BYTES: usize = 8;

/// Encodes one complete frame: the length prefix, then `postcard((path, elements))`.
fn encode_frame(session: &SessionId, packet: &Packet) -> Result<Vec<u8>> {
    let body = postcard::to_allocvec(&(session.path(), packet))?;
    let mut frame = Vec::with_capacity(LENGTH_PREFIX_BYTES + body.len());
    frame.extend_from_slice(&(body.len() as u64).to_le_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Decodes a frame body (the length prefix already removed by the codec).
fn decode_frame(root_tag: [u8; 32], body: &[u8]) -> Result<(SessionId, Packet)> {
    let (path, elements): (Vec<u32>, Vec<Element>) = postcard::from_bytes(body)?;
    Ok((SessionId::new(root_tag, path), Packet::new(elements)))
}
```

- `as u64`, not `usize::to_le_bytes` as today (`tcp.rs:61`): the codec is told to expect 8 bytes
  (`:133-136`), and `usize` is 4 bytes on a 32-bit target.
- The whole frame is one buffer. That is what lets a send be one atomic enqueue (§2.7).
- Decoding to `Vec<Element>` keeps `Packet::new` in use; decoding to `Packet` would leave it dead
  code, which fails clippy under `-D warnings`.
- **Why the root tag is not sent.** It is the same for every frame of a connection (C-8), so it
  would add 32 bytes to each frame and tell the receiver nothing. Two executions cannot be mixed
  up on the wire, because each has its own connections (plan §6, C-8).
- **Why the header is this small.** postcard writes a tuple as its fields one after another, with
  no prefix (postcard 1.1.3, `src/ser/serializer.rs:269-271`); a slice as a varint length and then
  its elements (`:262-266`); a `u32` as a varint (`:143-146`); and a newtype struct such as
  `Packet` as its inner value (`:238-243`). The header is thus one byte for the path's length and
  one or more per index. Measured on the prototype: 1 byte in the root session, 3 bytes for the
  path `[1, 0]`, 5 bytes for `[300, 0, 7]`.
- **Why the length prefix is written by hand,** and not by a `FramedWrite` on the write side: the
  frame has to exist, complete, before it is queued, so that queueing it is the whole send. The
  writer task is then a plain byte pump. The prefix has to be what the reader's codec expects,
  8 bytes little-endian (`tcp.rs:133-136`), which is today's format.

### 2.6 Step 4 — Mailboxes

The plan's layout is `queues: HashMap<(PartyId, SessionId), _>` with `waiters: HashMap<SessionId,
Waker>`. The prototype groups by session instead *(departs from the plan)*, for three reasons: a
lookup needs no cloned key, retiring a session is one `remove`, and a `BTreeMap` iterates "sorted
by key" (std, `alloc/src/collections/btree/map.rs:2683`), so the first non-empty queue belongs to
the lowest ready sender, which is the `recv_any` rule.

```rust
/// What a writer task is asked to do.
enum WriterCmd {
    /// Write this fully encoded frame.
    Frame(Vec<u8>),
    /// Shut the write half down and report how it went.
    Close(oneshot::Sender<Result<()>>),
}

/// The state of the connection to one remote peer.
enum PeerState {
    Open,
    /// The peer closed its side cleanly.
    Closed,
    /// Reading, decoding or writing failed, with the reason.
    Failed(String),
}

/// The packets one session has received and not yet taken, and the receive waiting for more.
#[derive(Default)]
struct SessionMailbox {
    /// Per sender, in ascending id order: `recv_any` takes from the lowest ready sender.
    queues: BTreeMap<PartyId, VecDeque<Packet>>,
    /// At most one receive waits per session (invariant I2).
    waiter: Option<Waker>,
}

struct Mailboxes {
    sessions: HashMap<SessionId, SessionMailbox>,
    /// Remote peers only; the loop-back peer never closes.
    peers: HashMap<PartyId, PeerState>,
    /// Sessions whose handle was dropped: late frames for them are discarded.
    retired: HashSet<SessionId>,
}
```

```rust
impl Mailboxes {
    /// Queues `packet`, and returns the waker of the receive waiting in that session, if any.
    /// The caller wakes it after releasing the lock.
    fn deliver(&mut self, sender: PartyId, session: SessionId, packet: Packet) -> Option<Waker> {
        if self.retired.contains(&session) {
            return None;
        }
        let mailbox = self.sessions.entry(session).or_default();
        mailbox.queues.entry(sender).or_default().push_back(packet);
        mailbox.waiter.take()
    }

    /// Records that the connection to `peer` ended, and returns every waiting receive's waker.
    fn end_peer(&mut self, peer: PartyId, end: PeerState) -> Vec<Waker> {
        // The first end is kept: a reader's EOF must not hide a writer's earlier failure.
        if let Some(state @ PeerState::Open) = self.peers.get_mut(&peer) {
            *state = end;
        }
        self.sessions
            .values_mut()
            .filter_map(|mailbox| mailbox.waiter.take())
            .collect()
    }

    fn peer_error(&self, peer: PartyId) -> Option<NetworkError> {
        match self.peers.get(&peer)? {
            PeerState::Open => None,
            PeerState::Closed => Some(NetworkError::ConnectionClosed(Some(peer))),
            PeerState::Failed(reason) => Some(NetworkError::PeerFailed {
                peer,
                reason: reason.clone(),
            }),
        }
    }

    fn poll_recv_from(
        &mut self,
        session: &SessionId,
        sender: PartyId,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Packet>> {
        let queued = self
            .sessions
            .get_mut(session)
            .and_then(|mailbox| mailbox.queues.get_mut(&sender))
            .and_then(VecDeque::pop_front);
        if let Some(packet) = queued {
            return Poll::Ready(Ok(packet));
        }
        // Only once the queue is drained does the peer's end show.
        if let Some(error) = self.peer_error(sender) {
            return Poll::Ready(Err(error));
        }
        self.sessions.entry(session.clone()).or_default().waiter = Some(cx.waker().clone());
        Poll::Pending
    }

    fn poll_recv_any(
        &mut self,
        session: &SessionId,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(PartyId, Packet)>> {
        if let Some(mailbox) = self.sessions.get_mut(session) {
            for (sender, queue) in &mut mailbox.queues {
                if let Some(packet) = queue.pop_front() {
                    return Poll::Ready(Ok((*sender, packet)));
                }
            }
        }
        if self
            .peers
            .values()
            .all(|state| !matches!(state, PeerState::Open))
        {
            return Poll::Ready(Err(NetworkError::ConnectionClosed(None)));
        }
        self.sessions.entry(session.clone()).or_default().waiter = Some(cx.waker().clone());
        Poll::Pending
    }

    fn retire(&mut self, session: &SessionId) {
        self.sessions.remove(session);
        self.retired.insert(session.clone());
    }
}

fn lock(mailboxes: &Mutex<Mailboxes>) -> MutexGuard<'_, Mailboxes> {
    mailboxes
        .lock()
        .expect("nothing panics while it holds the mailbox lock")
}
```

Why it is built this way:

- **Why a receive cannot miss a packet.** `poll_recv_from` looks at the queue and stores the waker
  under one acquisition of the lock, and `deliver` pushes the packet and takes the waker under the
  same lock. A delivery therefore comes either before the look, and the receive finds the packet,
  or after the waker is stored, and the delivery finds the waker. There is no moment in between.
  This is the lost wake-up that Phase 1 removed from the simulator (B3), whose receive has the
  same shape (`recv.rs:101-116`). *(AI reasoning; re-derive from `poll_recv_from` and `deliver`
  above.)*
- **Why a wake-up for nothing is harmless.** A wake means only "look again": the receive runs its
  `poll` once more and, if its own queue is still empty, stores a new waker. That is why `deliver`
  need not know which sender a receive is waiting for, and why `end_peer` can simply wake every
  session.
- **Why one waker per session is enough.** Invariant I2: a handle is not `Clone` and every receive
  borrows it mutably, so a session has at most one receive in flight
  (`src/net/mod.rs:596-601`).
- **Why the waker is taken, not cloned.** One wake is all a waiting receive needs. A waker left in
  place would wake the task again at every later frame, for a receive that has already returned.
- **Why wakers are returned, and woken after the lock is released.** On a multi-threaded runtime
  the woken task can start on another worker at once, and the first thing it does is take this
  lock. Woken inside the critical section, it would wait for the very code that woke it. *(AI
  reasoning; the general rule of not calling out while holding a lock. No source cited.)*
- **Why the pop and the `Ready` are in the same `poll`.** A receive future that is dropped between
  polls has taken nothing. So a timed-out or cancelled receive cannot lose a packet (plan §5.3,
  *Receive futures*).
- **Why the queue is looked at before the peer's state.** A peer's end then shows only once its
  queue is drained, so packets that arrived before the close are still delivered.
- **Why `std::sync::Mutex`, not tokio's.** Tokio's own advice: "it is ok and often preferred to
  use the ordinary `Mutex` from the standard library in asynchronous code"; what the async mutex
  adds is "the ability to keep it locked across an `.await` point" (tokio 1.53.1,
  `src/sync/mutex.rs:26-30`). Nothing here awaits while it holds the lock.
- **Why a failure is kept as text.** It has to be handed out more than once, and `NetworkError`
  is not `Clone`: it wraps `io::Error` among others.
- **Why `end_peer` keeps the first end.** A reader's EOF that follows a writer's failure must not
  turn `Failed` into `Closed`.
- **Why `retired` exists, and why it holds exact sessions.** Under fail-fast the local branch is
  dropped while the other parties may go on sending to its session. Without `retired` those frames
  would be queued for ever, for a handle that no longer exists. It holds exact sessions, not whole
  subtrees, because a child handle is a value of its own and may outlive its parent's handle. The
  set only grows; the plan notes how to prune it if that ever matters (§10).
- **Why a mailbox is created on demand** (`entry(..).or_default()` in `deliver`). That is the
  whole of the early-arrival case, T5: a frame for a session the receiver has not forked yet finds
  no mailbox, makes one, and waits in it.

### 2.7 Step 5 — The two tasks

The tasks, and `create` in §2.8, name a peer's TLS stream through a type alias, defined at the top
of `tcp.rs`:

```rust
type Socket = TlsStream<TcpStream>;
```

`TlsStream` is `tokio_rustls::TlsStream`, an enum over the client and the server side of a TLS
connection. A party connects as the client to the peers with a lower id and accepts the peers with a
higher id as the server (`tcp.rs:196-231` at `34c2856`), so both kinds of stream occur.
`tokio::io::split` turns one `Socket` into a `ReadHalf<Socket>` for the reader task and a
`WriteHalf<Socket>` for the writer task (§2.8).

```rust
/// Reads frames from one peer until its stream ends, filing each packet under its session.
async fn read_loop(
    peer: PartyId,
    mut frames: FramedRead<ReadHalf<Socket>, LengthDelimitedCodec>,
    mailboxes: Arc<Mutex<Mailboxes>>,
    root_tag: [u8; 32],
) {
    let end = loop {
        match frames.next().await {
            Some(Ok(body)) => match decode_frame(root_tag, &body) {
                Ok((session, packet)) => {
                    let waker = lock(&mailboxes).deliver(peer, session, packet);
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                }
                // A frame that does not decode cannot be attributed to a session.
                Err(error) => break PeerState::Failed(error.to_string()),
            },
            Some(Err(error)) => break PeerState::Failed(error.to_string()),
            None => break PeerState::Closed,
        }
    };
    let wakers = lock(&mailboxes).end_peer(peer, end);
    wakers.into_iter().for_each(Waker::wake);
}

/// Writes the frames queued for one peer, in queue order, until told to close or until every
/// sender is gone.
async fn write_loop(
    peer: PartyId,
    mut socket: WriteHalf<Socket>,
    mut commands: mpsc::UnboundedReceiver<WriterCmd>,
    mailboxes: Arc<Mutex<Mailboxes>>,
) {
    while let Some(command) = commands.recv().await {
        match command {
            WriterCmd::Frame(frame) => {
                let written = async {
                    socket.write_all(&frame).await?;
                    socket.flush().await
                };
                if let Err(error) = written.await {
                    let wakers =
                        lock(&mailboxes).end_peer(peer, PeerState::Failed(error.to_string()));
                    wakers.into_iter().for_each(Waker::wake);
                    return;
                }
            }
            WriterCmd::Close(ack) => {
                let result = socket.shutdown().await.map_err(NetworkError::from);
                let _ = ack.send(result);
                return;
            }
        }
    }
    // Every sender is gone: the core was dropped without `close()`.
    let _ = socket.shutdown().await;
}
```

- **Why the tasks get the mailboxes and not the core.** The core owns what ends the tasks: the
  senders of the writers' queues and the readers' abort handles. A task that held the core would
  keep it alive, and could itself not end until the core was dropped. With only the mailboxes'
  `Arc`, dropping the last handle drops the core (plan §5.3, *Ownership*).
- **Why a reader is aborted while a writer is left to finish.** A reader waits in `frames.next()`
  until its peer closes, which may be never, and meanwhile it owns the read half. Only an abort
  ends it. A writer ends by itself: once every sender is gone, `recv()` still returns the frames
  already queued, and then `None` (tokio 1.53.1, `src/sync/mpsc/unbounded.rs:115-118`). So it
  writes what `send_to` has already reported as sent, and then closes. Aborting it would throw
  those frames away.
- **Why the final `shutdown()`.** It is what sends TLS `close_notify` (tokio-rustls 0.26.5,
  `src/client.rs:512-514`, `src/server.rs:526`). Without it the peer does not see a clean end.
  Measured on the prototype with the write half merely dropped: the peer's receive returned
  `PeerFailed` carrying rustls's "peer closed connection without sending TLS close_notify"
  (rustls 0.23.45, `src/conn.rs:302`), where T8 (e) expects `ConnectionClosed`.
- **Why `Close` is a command in the same queue, with a `oneshot`.** Being in the queue, it is
  handled after every frame queued before it, which is "flush, then close" with no code of its
  own. The `oneshot` lets `close()` wait until that has happened, and carries the shutdown's
  error back, as today's `close()` returns it (`tcp.rs:364-370`).
- **Why an undecodable frame fails the whole peer.** It cannot be attributed to a session, and
  after it the position in the stream cannot be trusted either.
- After a write error the writer task returns, which drops its receiver. Frames queued behind the
  failed one are lost, and the next send to that peer gets the stored failure. This is the plan's
  "a write error surfaces at the next operation on that peer".

### 2.8 Step 6 — Core, handle, lifecycle

```rust
/// What every handle of one network shares.
struct TcpCore {
    /// Every party, this node included, in ascending id order.
    parties: Vec<PartyId>,
    /// One writer task per remote peer.
    writers: HashMap<PartyId, mpsc::UnboundedSender<WriterCmd>>,
    mailboxes: Arc<Mutex<Mailboxes>>,
    /// One reader task per remote peer.
    readers: Vec<AbortHandle>,
}

impl Drop for TcpCore {
    fn drop(&mut self) {
        for reader in &self.readers {
            reader.abort();
        }
        // `writers` is dropped right after this body: each writer task then sees its channel
        // close, writes what is still queued, shuts its write half down and exits.
    }
}

/// A real-network backend connecting this node to every party over mutually authenticated TLS.
pub struct TcpNetwork {
    /// ID of the party running this node.
    local: PartyId,
    core: Arc<TcpCore>,
    /// The session this handle sends and receives in.
    session: SessionId,
    /// The fork index of the next session forked from this handle.
    next_child: u32,
}

impl Drop for TcpNetwork {
    fn drop(&mut self) {
        // Not `lock()`: panicking in a destructor during an unwind aborts the process.
        if let Ok(mut mailboxes) = self.core.mailboxes.lock() {
            mailboxes.retire(&self.session);
        }
    }
}
```

- **Why `AbortHandle`, not `JoinHandle`.** The core never waits for a reader; it only has to stop
  it, and that is what an `AbortHandle` is: "an owned permission to abort a spawned task, without
  awaiting its completion" (tokio 1.53.1, `src/runtime/task/abort.rs:5-8`). Keeping `JoinHandle`s
  would not stop anything by itself either: a `JoinHandle` "detaches the associated task when it
  is dropped" (`src/runtime/task/join.rs:18`).
- **Why the readers have to be aborted for a socket to close.** `tokio::io::split` returns two
  halves that share the stream through an `Arc` (`src/io/split.rs:19-24`), so the stream is closed
  only once both are gone. The writer drops its half when it finishes; the reader's half goes when
  the reader is aborted (To-do amendment, *Root-handle `Drop`*).
- **Why `Drop for TcpNetwork` uses `if let Ok(..)`, not `expect`.** A destructor may run during an
  unwind, and "if the `drop()` panics in that situation (a 'double panic'), this will likely abort
  the program" (std, `core/src/ops/drop.rs:221-229`).
- `Drop for TcpNetwork` retires the handle's session, root or forked. **Why the root session
  too:** a forked handle may outlive the root handle, and frames for the root session would then
  pile up with nobody left to receive them. Because the type now implements `Drop`, its fields
  cannot be moved out of it.

**`create`** (`tcp.rs:175-246`). Establish every connection first, into a
`HashMap<PartyId, Socket>`, and spawn nothing until all of them exist. **Why in two steps:** if a
later `connect` or `accept` failed after tasks had been spawned, `create` would return its error
and those reader tasks would be left running, each holding a socket open. Then:

```rust
// Check that all parties are present
let parties: Vec<PartyId> = (0..n_parties).map(PartyId::from).collect();
for party in &parties {
    if party.as_usize() != id && !sockets.contains_key(party) {
        return Err(NetworkError::PartyNotFound(*party));
    }
}

// Step 2: one reader task and one writer task per remote peer.
let mailboxes = Arc::new(Mutex::new(Mailboxes {
    sessions: HashMap::new(),
    peers: sockets
        .keys()
        .map(|peer| (*peer, PeerState::Open))
        .collect(),
    retired: HashSet::new(),
}));
let mut writers = HashMap::new();
let mut readers = Vec::new();
for (peer, socket) in sockets {
    let (read_half, write_half) = tokio::io::split(socket);
    let codec = LengthDelimitedCodec::builder()
        .little_endian()
        .length_field_length(LENGTH_PREFIX_BYTES)
        .new_codec();
    let frames = FramedRead::new(read_half, codec);
    let reader = tokio::spawn(read_loop(peer, frames, Arc::clone(&mailboxes), root_tag));
    readers.push(reader.abort_handle());

    let (commands, queue) = mpsc::unbounded_channel();
    tokio::spawn(write_loop(peer, write_half, queue, Arc::clone(&mailboxes)));
    writers.insert(peer, commands);
}

Ok(Self {
    local: PartyId::from(id),
    core: Arc::new(TcpCore {
        parties,
        writers,
        mailboxes,
        readers,
    }),
    session: SessionId::root(root_tag),
    next_child: 0,
})
```

`tokio::spawn` here does not break the rule in `Network::send_many`'s docs
(`src/net/mod.rs:479-481`). That rule protects the trait's default methods, which the simulator
must be able to drive; `TcpNetwork` already needs a tokio runtime. The plan asks for a sentence in
that doc comment saying so (§5.3, last paragraph).

**Why the loop-back peer has no task and no channel.** There is no I/O to wait for. A send to
oneself is a `deliver` into one's own mailbox (§2.9). The packet is cloned, not serialized, so its
type labels survive, as they do through today's in-process channel (`tcp.rs:148-152`).

The inherent `send` and `recv` keep their behaviour, written over the party list:

```rust
/// Sends a packet to every party in the network, including this node's own loop-back channel.
/// Returns the number of payload bytes written for the last peer.
pub async fn send(&mut self, packet: &Packet) -> Result<usize> {
    let mut bytes_sent = 0;
    for party in self.core.parties.clone() {
        bytes_sent = self.send_to(party, packet).await?;
    }
    Ok(bytes_sent)
}

/// Receives one packet from each party in the network, including this node's own loop-back
/// channel, ordered by ascending party ID.
pub async fn recv(&mut self) -> Result<Vec<Packet>> {
    let mut packets = Vec::new();
    for party in self.core.parties.clone() {
        packets.push(self.recv_from(party).await?);
    }
    Ok(packets)
}

fn check_party(&self, party: PartyId) -> Result<()> {
    if party.as_usize() < self.core.parties.len() {
        Ok(())
    } else {
        Err(NetworkError::PartyNotFound(party))
    }
}
```

### 2.9 Step 7 — The `Network` and `ForkNetwork` impls

```rust
impl Network for TcpNetwork {
    fn party_ids(&self) -> Vec<PartyId> {
        self.core.parties.clone()
    }

    async fn recv_any(&mut self) -> Result<(PartyId, Packet)> {
        let (mailboxes, session) = (&self.core.mailboxes, &self.session);
        poll_fn(|cx| lock(mailboxes).poll_recv_any(session, cx)).await
    }

    async fn recv_any_with_timeout(&mut self, timeout: Duration) -> Result<(PartyId, Packet)> {
        let (mailboxes, session) = (&self.core.mailboxes, &self.session);
        let receive = poll_fn(|cx| lock(mailboxes).poll_recv_any(session, cx));
        tokio::time::timeout(timeout, receive)
            .await
            .unwrap_or(Err(NetworkError::Timeout(None)))
    }

    fn other(&self) -> Result<PartyId> {
        if self.core.parties.len() != 2 {
            Err(NetworkError::ExpectedTwoNodeNet(self.core.parties.len()))
        } else {
            Ok(PartyId::from(1 - self.local.as_usize()))
        }
    }

    /// Sends a packet of information to a given party.
    async fn send_to(&mut self, party_id: PartyId, packet: &Packet) -> Result<usize> {
        self.check_party(party_id)?;
        if party_id == self.local {
            // Loop-back: straight into this session's own mailbox.
            let waker = lock(&self.core.mailboxes).deliver(
                self.local,
                self.session.clone(),
                packet.clone(),
            );
            if let Some(waker) = waker {
                waker.wake();
            }
            return Ok(packet.size());
        }
        if let Some(error @ NetworkError::PeerFailed { .. }) =
            lock(&self.core.mailboxes).peer_error(party_id)
        {
            return Err(error);
        }
        let frame = encode_frame(&self.session, packet)?;
        self.core.writers[&party_id]
            .send(WriterCmd::Frame(frame))
            .map_err(|_| NetworkError::ConnectionClosed(Some(party_id)))?;
        Ok(packet.size())
    }

    async fn send_many(&mut self, messages: &[(PartyId, Packet)]) -> Result<()> {
        for (party_id, _) in messages {
            self.check_party(*party_id)?;
        }
        for (party_id, packet) in messages {
            self.send_to(*party_id, packet).await?;
        }
        Ok(())
    }

    async fn recv_from_with_timeout(
        &mut self,
        party_id: PartyId,
        timeout: Duration,
    ) -> Result<Packet> {
        self.check_party(party_id)?;
        let (mailboxes, session) = (&self.core.mailboxes, &self.session);
        let receive = poll_fn(|cx| lock(mailboxes).poll_recv_from(session, party_id, cx));
        tokio::time::timeout(timeout, receive)
            .await
            .unwrap_or(Err(NetworkError::Timeout(Some(party_id))))
    }

    /// Receives a packet from a given party.
    async fn recv_from(&mut self, party_id: PartyId) -> Result<Packet> {
        self.check_party(party_id)?;
        let (mailboxes, session) = (&self.core.mailboxes, &self.session);
        poll_fn(|cx| lock(mailboxes).poll_recv_from(session, party_id, cx)).await
    }

    /// Closes the network.
    async fn close(&mut self) -> Result<()> {
        if self.session.depth() > 0 {
            // A forked handle only gives up its own session.
            lock(&self.core.mailboxes).retire(&self.session);
            return Ok(());
        }
        let mut acks = Vec::new();
        for writer in self.core.writers.values() {
            let (ack, done) = oneshot::channel();
            // A writer that already exited has nothing left to close.
            if writer.send(WriterCmd::Close(ack)).is_ok() {
                acks.push(done);
            }
        }
        let mut result = Ok(());
        for done in acks {
            if let Ok(Err(error)) = done.await {
                result = result.and(Err(error));
            }
        }
        for reader in &self.core.readers {
            reader.abort();
        }
        // The readers are gone, so no peer's end will be seen any more. Record it here, and wake
        // whatever still waits in another session.
        let mut wakers = Vec::new();
        {
            let mut mailboxes = lock(&self.core.mailboxes);
            for peer in self.core.writers.keys() {
                wakers.extend(mailboxes.end_peer(*peer, PeerState::Closed));
            }
        }
        wakers.into_iter().for_each(Waker::wake);
        result
    }

    fn local_party(&self) -> PartyId {
        self.local
    }
}
```

- `send_to` never suspends. Dropping its future, which is what fail-fast does to a sibling
  branch, cannot leave half a frame on a stream that other sessions share (R-1).
- **Why `poll_fn`, and not named future types** like the simulator's `Recv`
  (`src/net/simulation/switchboard/recv.rs`). A receive only borrows the mailboxes and the session
  for as long as the call lasts, so a struct would have nothing to own. `poll_fn` turns the one
  `poll_*` method of `Mailboxes` into a future. *(AI choice; named types would work as well.)*
- **Why `tokio::time::timeout` around the mailbox future.** It is what the code does today around
  the stream (`tcp.rs:344`). Its rule for ties, "the future is polled before the timeout is
  checked" (tokio 1.53.1, `src/time/timeout.rs:24-26`), is the one the simulator's timed receive
  was written to match (`src/net/simulation/switchboard/mod.rs:370-372`). On a timeout the inner
  future is dropped, which is safe because a receive takes a packet and returns it in one `poll`.
  The session's waker stays registered; the next receive in that session replaces it.
- `send_many` keeps its up-front check of every target, and no longer needs `try_join_all`.
- `close()` on a forked handle only retires its session. On the root handle it closes the writers,
  aborts the readers, and marks every peer closed (decision 6). **Why it waits for every ack
  before aborting the readers:** the frames queued before the `Close`, and the `close_notify`,
  are on the wire by then. Of several errors it returns the first.

```rust
impl ForkNetwork for TcpNetwork {
    fn fork(&mut self) -> Self {
        let session = self.session.with_child(self.next_child);
        self.next_child += 1;
        Self {
            local: self.local,
            core: Arc::clone(&self.core),
            session,
            next_child: 0,
        }
    }

    fn session(&self) -> &SessionId {
        &self.session
    }
}
```

This is `SimNetwork::fork` (`src/net/simulation/network.rs:176-187`) with the core shared instead
of the switchboard.

### 2.10 What changes for users

For the CHANGELOG and the migration notes:

- The frame body, so 0.14.x and 0.15.0 cannot talk to each other.
- `recv_any` returns the lowest ready sender (G6; To-do amendment).
- `send_to` returns once the frame is queued, and a write error is reported by the next operation
  on that peer (G6).
- `party_ids()` is in ascending order. Today it is `HashMap` key order (`tcp.rs:270-272`).
  *(addition)*
- After the root `close()`, both halves of each stream are dropped. Today the write halves stay
  alive until the `TcpNetwork` is dropped (`:364-370`).
- `NetworkError::SendError` (`src/net/mod.rs:120`) is no longer produced.
- `NetworkError::PeerFailed` (decision 1, decided 2026-11-10). A read error, an undecodable frame
  or a write error on a peer is reported as `PeerFailed` by every later receive from that peer and
  send to it, where 0.14.x returned `IoError` or `SerializationError`.

Docs to touch: the `tcp.rs` module and struct docs, the `#[allow(dead_code)]` on `root_tag`
(`:96-98`, the field goes away), `Network::send_many`'s doc, and `docs/roadmap.md` §7, whose note
that a task per peer "was unnecessary" now contradicts C-4 (To-do amendment).

### 2.11 To-do bullets

| To-do bullet | Section |
| --- | --- |
| Skeleton | §2.3 |
| Tests: T3, T5 over TCP; T8 | §2.4 |
| Frame body, one buffer, one `WriterCmd::Frame` | §2.5, §2.9 |
| `TcpCore`, reader and writer tasks, `Mailboxes`, loop-back | §2.6–§2.8 |
| Peer failure | §2.6 (`end_peer`, `peer_error`), §2.7 |
| Lifecycle | §2.8, §2.9 (`close`) |
| `ForkNetwork` impl | §2.9 |
| Exit | §0, §1 |
| Amendment: sends not cancel-safe | §2.9; T8 (c) |
| Amendment: root-handle `Drop` | §2.8; T8 (e) |

---

## 3. Phase 4 — session-aware tracing

### 3.1 What is wrong today

Both the bandwidth tree (`src/net/simulation/simulator.rs:324-350`) and the trace `Display`
(`src/net/simulation/mod.rs:211-238`) treat the begin and end markers as one stack per party.
Under a `Join` the branches interleave in the trace, and the stack nests one branch inside the
other. This is what the current code prints for a `Join` of `Left` and `Right` followed by
`After` (real output, from the test of §3.4 run before the implementation):

```text
JoinThenAfter {
    Join {
        Left {
            [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
            Right {
                [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
                [     0.050s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
            }
            [     0.050s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
        }
    }
    After {
```

And the folded bandwidth tree of `Join((PassiveRandShr, PassiveRandDoubleShr))` puts the second
protocol's 40 bytes under the first:

```text
<simulation>;Both;Join;PassiveRandShr;PassiveDealLinearShr 40
<simulation>;Both;Join;PassiveRandShr;PassiveDealLinearShr;PassiveRandDoubleShr;PassiveDealLinearShr 40
```

After this phase the same two runs give:

```text
JoinThenAfter {
    Join {
        Left [session 0] {
            [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
            [     0.050s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
        }
        Right [session 1] {
            [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
            [     0.050s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
        }
    }
    After {
```

```text
<simulation>;Both;Join;PassiveRandDoubleShr;PassiveDealLinearShr 40
<simulation>;Both;Join;PassiveRandShr;PassiveDealLinearShr 40
```

### 3.2 Decisions that are yours

1. **What a malformed trace returns.** The prototype adds a variant to `SimulationError`
   (`#[non_exhaustive]`, `src/net/simulation/mod.rs:106`), returned by `bandwidth_tree_for`:

   ```rust
   /// The trace's protocol scopes do not nest: a scope ends that is not the one open in its
   /// session.
   #[error("malformed trace at event {event_index}: {reason}")]
   MalformedTrace {
       /// Index of the offending event in the trace.
       event_index: usize,
       /// What is wrong with it.
       reason: String,
   },
   ```

   The `Display` does not fail on such a trace: it prints the offending `ProtocolEnd` as a plain
   event line and carries on. **Why:** formatting "is considered an infallible operation", and an
   implementation "should not return this `Error` unless they received it from their `Formatter`"
   (std, `core/src/fmt/mod.rs:76-84`). A trace that does not nest is also the one most worth
   looking at.
2. **How strict the end check is.** The plan says: pop the session's stack and check the name
   (§5.6). The prototype does that. A consequence: a scope dropped in the *same* session as its
   caller, for example a sub-protocol's `execute` wrapped in a timeout, leaves its parent's end
   looking mismatched. No such trace arises from the combinators, which drop only scopes in child
   sessions.
3. **What a scope with no end looks like.** The prototype prints `} (not finished)`. Today an
   unbalanced trace prints no closing brace. **Why a brace at all:** the blocks after it keep their
   indentation, and the note says the end was never recorded.
4. **What `SimulationTrace::session` keeps.** The prototype keeps the session and everything below
   it, so it selects one branch including its nested joins, and drops the events that carry no
   session (`Start`, `Stop`, `Output`). The alternative is the one session only. **Why the
   subtree:** a branch of a `Join` is its session together with whatever nested combinators
   forked below it.
5. **The label.** `Name [session 0.2] {`, as in the plan. **Why only on the first scope of a
   branch:** a scope is labelled when its session is not the session of the scope around it and
   is not a root session. In a sequential trace every scope is in the root session, so no label
   is printed and the text is what it was.

### 3.3 Step 1 — Skeleton

`SimulationTrace::session` with a `todo!()` body, and the error variant of decision 1. With only
these two, the tests of the next step compile and all four fail (checked).

### 3.4 Step 2 — Tests (T6)

A new file, `tests/trace_tree.rs`. Everything it needs is public: `bandwidth_tree_for`,
`SimulationTrace::new`, and the fields of `SimulationOutcome`.

The tree's fields are private, so the tests read it through `write_folded`:

```rust
/// The folded bandwidth tree of `party`, as a map from each path to its bytes.
fn folded<O>(outcome: &SimulationOutcome<O>, party: PartyId) -> BTreeMap<String, usize> {
    let mut out = Vec::new();
    outcome
        .bandwidth_tree_for(party)
        .unwrap()
        .write_folded(&mut out)
        .unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| {
            let (path, bytes) = line.rsplit_once(' ').unwrap();
            (path.to_string(), bytes.parse().unwrap())
        })
        .collect()
}
```

**T6 (a): each branch gets its own bytes.** The protocols, then the test. The comparison is
between whole subtrees, because the bytes are sent by sub-protocols of the two protocols, not by
the protocols themselves:

```rust
/// Runs the two preprocessing protocols in one `Join`.
struct Both;

impl<E: RandEnvironment + ForkEnvironment> Protocol<E> for Both {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        Join((
            PassiveRandShr::<1, F>::new(T, parties())?,
            PassiveRandDoubleShr::<1, F>::new(T, parties())?,
        ))
        .execute(env)
        .await?;
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Both")
    }
}

/// Runs one of the two protocols alone, discarding its output.
struct Alone(bool);

impl<E: RandEnvironment> Protocol<E> for Alone {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        if self.0 {
            PassiveRandShr::<1, F>::new(T, parties())?
                .execute(env)
                .await?;
        } else {
            PassiveRandDoubleShr::<1, F>::new(T, parties())?
                .execute(env)
                .await?;
        }
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Alone")
    }
}
```

```rust
/// T6 (a): in a `Join`, the tree gives each branch the bytes that protocol sends when it runs
/// alone, under the `Join` node: the tree of the joined run is the two trees of the runs alone,
/// side by side.
#[test]
fn t6_each_branch_of_a_join_gets_its_own_bytes() {
    let config = SimpleNetworkConfig::default;
    let joined = simulate(config(), parties(), |_| Both, seeded_env, vec![]);
    let first = simulate(config(), parties(), |_| Alone(true), seeded_env, vec![]);
    let second = simulate(config(), parties(), |_| Alone(false), seeded_env, vec![]);

    for p in parties() {
        let mut expected = BTreeMap::new();
        for alone in [&first, &second] {
            for (path, bytes) in folded(alone, p) {
                let below_alone = path.strip_prefix("<simulation>;Alone;").unwrap();
                expected.insert(format!("<simulation>;Both;Join;{below_alone}"), bytes);
            }
        }
        for name in ["PassiveRandShr", "PassiveRandDoubleShr"] {
            let prefix = format!("<simulation>;Both;Join;{name}");
            assert!(
                expected.keys().any(|path| path.starts_with(&prefix)),
                "party {p}: {name} sent nothing when run alone"
            );
        }
        assert_eq!(folded(&joined, p), expected, "party {p}");
    }
}
```

**T6 (b) and (c)** use hand-built traces, which give exact control over the order of events:

```rust
fn root() -> SessionId {
    SessionId::root(SIMULATION_ROOT_TAG)
}

fn begin(session: &SessionId, name: &'static str) -> Event {
    Event::ProtocolBegin {
        timestamp: Duration::ZERO,
        session: session.clone(),
        protocol_name: ProtocolId::from(name),
    }
}

fn end(session: &SessionId, name: &'static str) -> Event {
    Event::ProtocolEnd {
        timestamp: Duration::ZERO,
        session: session.clone(),
        protocol_name: ProtocolId::from(name),
    }
}

/// A send of `size` bytes from party 0 to party 1.
fn send(session: &SessionId, size: usize) -> Event {
    Event::SendData {
        timestamp: Duration::ZERO,
        session: session.clone(),
        link: Link::new(party(0), party(1)),
        size,
        content_count: vec![("unknown elem.", 1)],
    }
}

/// An outcome holding one hand-built trace, for party 0.
fn outcome_of(events: Vec<Event>) -> SimulationOutcome<()> {
    SimulationOutcome {
        outputs: HashMap::new(),
        traces: HashMap::from([(party(0), SimulationTrace::new(events))]),
    }
}
```

```rust
/// T6 (b): a `ProtocolEnd` whose name is not the scope open in its session is reported.
#[test]
fn t6_an_end_with_the_wrong_name_is_a_malformed_trace() {
    let outcome = outcome_of(vec![
        begin(&root(), "A"),
        send(&root(), 1),
        end(&root(), "B"),
    ]);
    let result = outcome.bandwidth_tree_for(party(0));
    assert!(
        matches!(
            result,
            Err(SimulationError::MalformedTrace { event_index: 2, .. })
        ),
        "expected MalformedTrace at event 2"
    );

    let outcome = outcome_of(vec![end(&root(), "A")]);
    assert!(matches!(
        outcome.bandwidth_tree_for(party(0)),
        Err(SimulationError::MalformedTrace { event_index: 0, .. })
    ));
}
```

```rust
/// T6 (c): a branch that a fail-fast `Join` dropped never records its end. Closing the `Join`
/// closes it, so what the party sends afterwards is not counted under the dropped branch.
#[test]
fn t6_a_cancelled_branch_is_closed_with_its_join() {
    let (a, b) = (root().with_child(0), root().with_child(1));
    let outcome = outcome_of(vec![
        begin(&root(), "Join"),
        begin(&a, "A"),
        send(&a, 10),
        begin(&b, "B"),
        send(&b, 20),
        end(&b, "B"),
        // `A` is dropped here: no `ProtocolEnd`.
        end(&root(), "Join"),
        begin(&root(), "C"),
        send(&root(), 40),
        end(&root(), "C"),
    ]);
    assert_eq!(
        folded(&outcome, party(0)),
        BTreeMap::from([
            ("<simulation>;Join;A".to_string(), 10),
            ("<simulation>;Join;B".to_string(), 20),
            ("<simulation>;C".to_string(), 40),
        ])
    );
}
```

On the current parser, (c) gives `Join;A 10`, `Join;A;B 20` and `Join;C 40`.

**Rendering and `session`.** The network is instantaneous, so every timestamp is zero and the
expected text does not depend on the delay model. The first version of this test used the default
network; Phase 5 then moved its timestamps and broke it.

```rust
/// One round: sends one small packet to every other party, then receives one from each.
struct Round(&'static str);

impl<E: Environment> Protocol<E> for Round {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        let network = env.network_mut();
        let local = network.local_party();
        let others: Vec<PartyId> = network
            .party_ids()
            .into_iter()
            .filter(|&p| p != local)
            .collect();
        let mut packet = Packet::empty();
        packet.write(&0u64)?;
        for &other in &others {
            network.send_to(other, &packet).await?;
        }
        for &other in &others {
            network.recv_from(other).await?;
        }
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from(self.0)
    }
}

/// `Left` and `Right` in one `Join`, then `After` alone.
struct JoinThenAfter;

impl<E: Environment + ForkEnvironment> Protocol<E> for JoinThenAfter {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        Join((Round("Left"), Round("Right"))).execute(env).await?;
        Round("After").execute(env).await
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("JoinThenAfter")
    }
}
```

```rust
/// The trace of a `Join` renders each branch as its own block, labelled with its session, with
/// that branch's events inside it; and `session` keeps one branch. The links are instantaneous,
/// so every timestamp is zero and the expected text does not depend on the delay model.
#[test]
fn t6_a_join_renders_one_labelled_block_per_branch() {
    let instant = ChannelConfigBuilder::default()
        .net_type(NetworkType::Instant)
        .build()
        .unwrap();
    let outcome = simulate(
        SimpleNetworkConfig::from_channel_config(instant),
        vec![party(0), party(1)],
        |_| JoinThenAfter,
        |p, network| GeneralEnv::new(network, ChaCha20Rng::seed_from_u64(p.as_usize() as u64)),
        vec![],
    );
    let trace = &outcome.traces[&party(0)];
    let expected = "\
[     0.000s] START
JoinThenAfter {
    Join {
        Left [session 0] {
            [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
            [     0.000s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
        }
        Right [session 1] {
            [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
            [     0.000s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
        }
    }
    After {
        [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
        [     0.000s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
    }
}
[     0.000s] OUTPUT         []
[     0.000s] STOP";
    assert_eq!(trace.to_string(), expected);

    let right = trace.session(&root().with_child(1));
    let expected_right = "\
Right [session 1] {
    [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
    [     0.000s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
}";
    assert_eq!(right.to_string(), expected_right);
}
```

**T6 (d), sequential renderings byte-identical,** is `tests/golden.rs`, which already exists.

### 3.5 Step 3 — The tree

One private module, `src/net/simulation/tree.rs` (declare it `pub(crate) mod tree;` in
`simulation/mod.rs`). Both consumers read the same tree, so they cannot disagree about nesting.

```rust
//! The tree of protocol scopes of one party's trace, rebuilt from its events.
//!
//! Both the bandwidth tree and the trace's `Display` are read off this tree, so they agree on how
//! scopes nest when the branches of a combinator interleave in the trace.

use std::collections::HashMap;
use std::fmt::{self, Formatter};

use crate::net::session::SessionId;
use crate::net::simulation::event::Event;
use crate::protocol::ProtocolId;

/// Index of the synthetic root node, which spans the whole trace.
pub(crate) const ROOT: usize = 0;

/// One thing recorded inside a scope, in the order of the trace.
pub(crate) enum Item {
    /// The event at this index of the trace.
    Event(usize),
    /// The scope of the node at this index of the arena.
    Child(usize),
}

/// One protocol scope: what lies between a `ProtocolBegin` and its `ProtocolEnd`.
pub(crate) struct Node {
    /// The protocol's name; `None` for the root.
    pub(crate) id: Option<ProtocolId>,
    /// The session the scope was opened in; `None` for the root.
    pub(crate) session: Option<SessionId>,
    /// Payload bytes sent in this scope itself, excluding its children and self-sends.
    pub(crate) self_bytes: usize,
    /// The scope's events and child scopes. They are pushed while walking the trace once, so they
    /// are in the order of the trace.
    pub(crate) items: Vec<Item>,
    /// Whether a `ProtocolEnd` closed the scope. False for the root, and for a scope whose
    /// protocol was dropped before it finished.
    pub(crate) ended: bool,
}

/// Something in a trace that does not fit a tree of scopes.
pub(crate) struct Anomaly {
    /// Index of the event in the trace.
    pub(crate) event_index: usize,
    pub(crate) reason: String,
}

pub(crate) struct TraceTree {
    /// The arena. A node's children always have a higher index than the node.
    pub(crate) nodes: Vec<Node>,
    pub(crate) anomalies: Vec<Anomaly>,
}

/// Whether `session` is strictly below `ancestor` in the session tree.
fn is_below(session: &SessionId, ancestor: &SessionId) -> bool {
    session.root_tag() == ancestor.root_tag()
        && session.path().len() > ancestor.path().len()
        && session.path().starts_with(ancestor.path())
}

impl TraceTree {
    /// Rebuilds the tree in one pass over `events`.
    pub(crate) fn build(events: &[Event]) -> Self {
        let mut nodes = vec![Node {
            id: None,
            session: None,
            self_bytes: 0,
            items: Vec::new(),
            ended: false,
        }];
        let mut anomalies = Vec::new();
        // The scopes open in each session, innermost last.
        let mut open: HashMap<SessionId, Vec<usize>> = HashMap::new();

        for (index, event) in events.iter().enumerate() {
            match event {
                Event::ProtocolBegin {
                    session,
                    protocol_name,
                    ..
                } => {
                    let parent = Self::innermost(&open, Some(session));
                    let node = nodes.len();
                    nodes.push(Node {
                        id: Some(*protocol_name),
                        session: Some(session.clone()),
                        self_bytes: 0,
                        items: Vec::new(),
                        ended: false,
                    });
                    nodes[parent].items.push(Item::Child(node));
                    open.entry(session.clone()).or_default().push(node);
                }
                Event::ProtocolEnd {
                    session,
                    protocol_name,
                    ..
                } => {
                    let innermost = open.get(session).and_then(|stack| stack.last().copied());
                    match innermost {
                        Some(node) if nodes[node].id == Some(*protocol_name) => {
                            nodes[node].ended = true;
                            open.get_mut(session).map(Vec::pop);
                            // Whatever is still open in the sessions below was dropped before it
                            // finished, by a combinator that failed fast.
                            open.retain(|other, _| !is_below(other, session));
                        }
                        Some(node) => {
                            let open_name = nodes[node].id.map(String::from).unwrap_or_default();
                            anomalies.push(Anomaly {
                                event_index: index,
                                reason: format!(
                                    "`{protocol_name}` ends in session {session}, where the \
                                     innermost open scope is `{open_name}`"
                                ),
                            });
                            nodes[node].items.push(Item::Event(index));
                        }
                        None => {
                            anomalies.push(Anomaly {
                                event_index: index,
                                reason: format!(
                                    "`{protocol_name}` ends in session {session}, where no scope \
                                     is open"
                                ),
                            });
                            let node = Self::innermost(&open, Some(session));
                            nodes[node].items.push(Item::Event(index));
                        }
                    }
                }
                Event::SendData {
                    session,
                    link,
                    size,
                    ..
                } => {
                    let node = Self::innermost(&open, Some(session));
                    // Self-sends never touch the wire, so they do not count as bandwidth.
                    if link.sender() != link.recipient() {
                        nodes[node].self_bytes += size;
                    }
                    nodes[node].items.push(Item::Event(index));
                }
                Event::ReceiveData { session, .. } => {
                    let node = Self::innermost(&open, Some(session));
                    nodes[node].items.push(Item::Event(index));
                }
                _ => {
                    let node = Self::innermost(&open, None);
                    nodes[node].items.push(Item::Event(index));
                }
            }
        }

        Self { nodes, anomalies }
    }

    /// The innermost scope open for `session`: the top of its own stack, or else of its nearest
    /// ancestor's, which is where the combinator that forked it is open. Events that carry no
    /// session belong to the innermost scope of a root session.
    fn innermost(open: &HashMap<SessionId, Vec<usize>>, session: Option<&SessionId>) -> usize {
        let Some(session) = session else {
            return open
                .iter()
                .filter(|(session, _)| session.depth() == 0)
                .filter_map(|(_, stack)| stack.last().copied())
                .max()
                .unwrap_or(ROOT);
        };
        let mut current = Some(session.clone());
        while let Some(session) = current {
            if let Some(&node) = open.get(&session).and_then(|stack| stack.last()) {
                return node;
            }
            current = session.parent();
        }
        ROOT
    }

    /// Writes the trace as nested blocks, one per scope.
    pub(crate) fn render(&self, events: &[Event], f: &mut Formatter<'_>) -> fmt::Result {
        self.render_items(ROOT, 0, events, f, &mut true)
    }

    fn render_items(
        &self,
        node: usize,
        depth: usize,
        events: &[Event],
        f: &mut Formatter<'_>,
        first_line: &mut bool,
    ) -> fmt::Result {
        for item in &self.nodes[node].items {
            start_line(f, depth, first_line)?;
            match item {
                Item::Event(index) => write!(f, "{}", events[*index])?,
                Item::Child(child) => {
                    let scope = &self.nodes[*child];
                    if let Some(id) = scope.id {
                        write!(f, "{id}")?;
                    }
                    // A scope names its session when it is the first scope of a branch: its
                    // session is not the one of the scope around it.
                    if let Some(session) = &scope.session {
                        if session.depth() > 0 && self.nodes[node].session.as_ref() != Some(session)
                        {
                            write!(f, " [session {session}]")?;
                        }
                    }
                    write!(f, " {{")?;
                    self.render_items(*child, depth + 1, events, f, first_line)?;
                    start_line(f, depth, first_line)?;
                    write!(f, "}}")?;
                    if !scope.ended {
                        write!(f, " (not finished)")?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Starts a line at `depth`: a line break, unless this is the first line, then the indentation.
fn start_line(f: &mut Formatter<'_>, depth: usize, first_line: &mut bool) -> fmt::Result {
    const INDENT: &str = "    ";
    if !std::mem::replace(first_line, false) {
        writeln!(f)?;
    }
    (0..depth).try_for_each(|_| f.write_str(INDENT))
}
```

How it implements the plan's §5.6, and why:

- **Why an arena, and not the recursive parse of today.** Today one call builds one node and
  returns when that node ends (`simulator.rs:324-350`). That works only while scopes end in the
  reverse of the order they began in. Under a `Join`, branch A's scope is still open when branch
  B's opens, gets events and closes, so the parser has to be able to add to *any* open node at
  any moment. With children owned by their parent (`Vec<ProtocolBandwidthTree>`) that would take
  several long-lived mutable references into one tree, which the borrow checker does not allow.
  Indices into a flat `Vec` are the usual way around it. The nested public type is built at the
  end, in one walk (§3.6).
- **Why one stack per session** (`open`). Inside one session scopes do nest: a session has one
  handle, and whatever runs holds it `&mut`, so its begin and end markers are properly bracketed.
  Across sessions they interleave. A stack is right per session and wrong per party.
- **Why the nearest ancestor's top is the right parent** for a branch's first scope
  (`innermost`). A combinator is called through `execute`, which records its `ProtocolBegin` in
  the parent session before anything is forked (`src/protocol/mod.rs:125-131`), and it holds the
  parent environment `&mut` until every branch has finished
  (`src/protocol/concurrent.rs:146-152`). So while the branches run, the top of the parent
  session's stack is the combinator's node, and nothing else can open or close there.
- **Why no timestamp is compared** (R-3). Local computation takes no virtual time, so events at
  the same instant are the rule, and sorting by time would reorder them. The order of the trace
  is the only order that is always defined. It also costs nothing: items are pushed during a
  single pass, so each node's `items` is already in trace order.
- **Why closing a scope clears the sessions below it.** A branch that fail-fast dropped never
  records its end. Its handle is a local of the combinator's `run` (`concurrent.rs:150`), so once
  the combinator's scope has ended nothing can run in those sessions any more. Their nodes keep
  `ended: false`, which the renderer shows as `(not finished)`.
- **Why events with no session** (`Start`, `Stop`, `Output`) **go to the innermost scope of a root
  session.** For a sequential trace that is where today's renderer prints them, by depth alone.
  It is what keeps the golden snapshot identical.
- **Why `.max()` in `innermost`.** A hand-built trace may hold root sessions of several tags, and
  `HashMap` iteration order changes from run to run. The choice among them has to be made by
  something stable: the scope opened last, which has the highest index.
- **Why both consumers read one tree.** Today each has its own stack logic
  (`simulator.rs:324-350`, `simulation/mod.rs:211-238`). One structure cannot disagree with itself
  about nesting.

### 3.6 Step 4 — Wiring

The bandwidth tree is copied out of the arena. `parse` goes away; the public type, its fields and
`write_folded` do not change:

```rust
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
            event_index: anomaly.event_index,
            reason: anomaly.reason.clone(),
        });
    }
    Ok(ProtocolBandwidthTree::from_scope(
        &tree,
        ROOT,
        ProtocolId::from("<simulation>"),
    ))
}
```

```rust
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
```

The `Display` becomes one line, and `session` is a filter:

```rust
impl std::fmt::Display for SimulationTrace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        tree::TraceTree::build(&self.0).render(&self.0, f)
    }
}
```

```rust
/// Returns the events of one branch: those recorded in `session` or in a session below it.
pub fn session(&self, session: &SessionId) -> SimulationTrace {
    let in_branch = |other: &SessionId| {
        other.root_tag() == session.root_tag() && other.path().starts_with(session.path())
    };
    let events = self.0.iter().filter(|event| match event {
        Event::SendData { session, .. }
        | Event::ReceiveData { session, .. }
        | Event::ProtocolBegin { session, .. }
        | Event::ProtocolEnd { session, .. } => in_branch(session),
        _ => false,
    });
    SimulationTrace(events.cloned().collect())
}
```

**Why `session` returns a new `SimulationTrace`,** and not an iterator over events: the result can
then be printed with the same `Display`, and handed to anything that takes a trace.

The long doc comment on the `Display` impl (`simulation/mod.rs:187-210`) stays, with a paragraph
added on branches and their labels.

### 3.7 A dropped branch, rendered

Real output of the prototype for `Join((Round("Slow"), Fail))` followed by `Round("After")`, where
the `Join` fails fast and drops `Slow` (run before Phase 5 was added):

```text
[     0.000s] START
FailFast {
    Join {
        Slow [session 0] {
            [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
        } (not finished)
        Fail [session 1] {
        }
    }
    After {
        [     0.000s] SEND           0 -> 1 (1 bytes: 1 unknown elem.)
        [     0.050s] RECV           0 <- 1 (1 bytes: 1 unknown elem.)
    }
}
[     0.050s] OUTPUT         []
[     0.050s] STOP
```

Its folded tree was `<simulation>;FailFast;After 1` and `<simulation>;FailFast;Join;Slow 1`.

### 3.8 Limits to document

- **Manual `fork`.** The tree is exact for `Join` and `JoinAll`. If a protocol forks by hand and
  keeps running scopes in the parent session while a child session runs, a branch's first scope
  attaches to whatever is on top of the parent's stack at that moment, and a scope ending in the
  parent session force-closes the child's. A fork event in the trace would fix this; the plan
  chose not to have one (§5.6). *(AI reasoning, not tested.)*
- **Recursion and same-session drops**: see decision 2.

### 3.9 Exit

- `cargo test --test trace_tree`: 4 of 4.
- `cargo test --test golden`: 11 of 11, with no regeneration. This is what shows sequential
  renderings and folded trees are byte-identical.
- The header comment of `tests/concurrent.rs` (`:8-10`) says the tree and the `Display` are not
  asserted on "until Phase 4". It becomes stale here.

| To-do bullet | Section |
| --- | --- |
| Skeleton | §3.3 |
| Tests: T6 | §3.4 |
| Arena parse, per-session stacks, ancestor fallback, name check, force-close | §3.5 |
| Tree-rendered `Display`; `SimulationTrace::session` | §3.6 |
| Exit | §3.9 |
| Amendment: merge by record index | §3.5 |

---

## 4. Phase 5 — link occupancy

### 4.1 The model

Today a message's arrival is `now + delay(size)`, with a clamp that keeps a link FIFO
(`src/net/simulation/switchboard/mod.rs:243-249`). Two messages sent at the same instant on one
link each get the whole bandwidth, and arrive together.

The plan (§5.7) splits the delay into the time the link is *busy* with the message and the time
the message merely *travels*, and makes the link serialize one message at a time:

```
start      = max(now, busy_until[link])
busy_until = start + serialization
arrival    = busy_until + propagation          // then the FIFO clamp, as today
```

Measured on the prototype, WAN preset, two 1 MiB packets sent at the same instant on one link:
the second arrives at 136.19 ms today and at 222.37 ms with occupancy, which is
`2 · 86.19 + 50`.

**Why two terms and not one.** A TCP stream behaves like a pipe. Bits enter it at the link's
throughput, and take the latency to cross it. A second message cannot start entering until the
first has finished entering, but it does not have to wait for the first to *arrive*. Only the time
spent entering, the serialization time, keeps other messages out. The propagation time is paid by
each message on its own. *(This is the textbook split into transmission delay and propagation
delay; stated from memory, no source cited.)*

**Why the link keeps `busy_until`, and the sender's clock is not touched.** A send does not hold
the sender up. On the simulator a party's clock advances only when something is delivered to it
(`src/net/simulation/simulator.rs:67-71`), and after Phase 3 a real `send_to` returns as soon as
the frame is queued. The waiting happens in the link, so the link is where the time is kept.

**Why per `Link`, not per session.** The sessions on a link share one TCP stream. It is the same
reason `last_arrivals` stayed per link in Phase 1 (plan §5.2).

### 4.2 Decisions that are yours

1. **A public `ChannelConfig::transmission`.** *(addition)* The plan has `ConfigDelay` override
   `Delay::transmission` by splitting the private `recv_time_tcp`. The prototype puts the split in
   a public method of `ChannelConfig`, beside `message_delay`, because the tests need the two
   terms to state what they expect.
2. **What T2 (b) should assert from now on** (§4.7). Its exact equality stops holding.
3. **On by default or opt-in** (C-6). That depends on the benchmark, which was not run (§4.8).

### 4.3 Step 1 — Skeleton

`Delay::transmission` with its real default body, and `ConfigDelay::transmission` and
`ChannelConfig::transmission` with `todo!()` bodies. *(Not compiled as a separate step. In the
prototype the split was written first and the switchboard left alone: then 1 of the 3 tests
passes, and the two timing tests fail with 136.18712 ms where 222.37424 ms is expected.)*

### 4.4 Step 2 — Tests (T9)

A new file, `tests/occupancy.rs`. The protocols:

```rust
const MIB: usize = 1024 * 1024;

fn party(index: usize) -> PartyId {
    PartyId::from(index)
}

fn env(party: PartyId, network: SimNetwork) -> GeneralEnv<SimNetwork, ChaCha20Rng> {
    GeneralEnv::new(network, ChaCha20Rng::seed_from_u64(party.as_usize() as u64))
}

fn mib_packet() -> Packet {
    let mut packet = Packet::empty();
    packet.write(&vec![0u8; MIB]).unwrap();
    packet
}

/// Party 0 sends 1 MiB to `target`, which receives it. Every other party does nothing.
struct Transfer {
    target: PartyId,
}

impl<E: Environment> Protocol<E> for Transfer {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        let local = env.network().local_party();
        if local == party(0) {
            env.network_mut()
                .send_to(self.target, &mib_packet())
                .await?;
        } else if local == self.target {
            env.network_mut().recv_from(party(0)).await?;
        }
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("Transfer")
    }
}

/// Two transfers to party 1 and one to party 2, all at once.
struct ThreeTransfers;

impl<E: Environment + ForkEnvironment> Protocol<E> for ThreeTransfers {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        Join((
            Transfer { target: party(1) },
            Transfer { target: party(1) },
            Transfer { target: party(2) },
        ))
        .execute(env)
        .await?;
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("ThreeTransfers")
    }
}

/// Party 0 sends two 1 MiB packets to party 1 one after the other, at the same instant.
struct BackToBack;

impl<E: Environment> Protocol<E> for BackToBack {
    type Output = ();

    async fn run(self, env: &mut E) -> Result<(), Error> {
        let network = env.network_mut();
        if network.local_party() == party(0) {
            network.send_to(party(1), &mib_packet()).await?;
            network.send_to(party(1), &mib_packet()).await?;
        } else {
            network.recv_from(party(0)).await?;
            network.recv_from(party(0)).await?;
        }
        Ok(())
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("BackToBack")
    }
}
```

Every party runs the same `Join` with the same three branches, so invariant I1 holds; a party
that is neither sender nor target runs a branch that returns at once. **Why the third transfer
goes to party 2:** it is the control. Sent at the same instant on another link, it must arrive as
if nothing else were being sent.

```rust
/// The serialization and propagation time of one 1 MiB packet on the link 0 -> `to`.
fn one_packet(config: &SimpleNetworkConfig, to: usize) -> (Duration, Duration) {
    config
        .channel_config(Link::new(party(0), party(to)))
        .transmission(mib_packet().size())
}

/// T9: two packets sent at the same instant on one link share it. The second arrives one
/// serialization time after the first, not together with it. A packet sent at that instant on
/// another link is not delayed.
#[test]
fn t9_concurrent_packets_on_one_link_share_its_bandwidth() {
    let config = SimpleNetworkConfig::wan();
    let (serialization, propagation) = one_packet(&config, 1);
    assert!(
        serialization > Duration::from_millis(10),
        "the test needs a visible serialization time"
    );

    let parties = vec![party(0), party(1), party(2)];
    let outcome = simulate(config.clone(), parties, |_| ThreeTransfers, env, vec![]);
    let metrics = outcome.metrics();
    assert_eq!(
        metrics.elapsed_time_for(party(1)).unwrap(),
        2 * serialization + propagation,
        "the second packet on the link 0 -> 1"
    );
    assert_eq!(
        metrics.elapsed_time_for(party(2)).unwrap(),
        serialization + propagation,
        "the only packet on the link 0 -> 2"
    );
}

/// The same holds within one session: the link is shared by whatever is sent on it.
#[test]
fn t9_back_to_back_packets_on_one_link_share_its_bandwidth() {
    let config = SimpleNetworkConfig::wan();
    let (serialization, propagation) = one_packet(&config, 1);

    let outcome = simulate(
        config.clone(),
        vec![party(0), party(1)],
        |_| BackToBack,
        env,
        vec![],
    );
    assert_eq!(
        outcome.metrics().elapsed_time_for(party(1)).unwrap(),
        2 * serialization + propagation
    );
}

/// The two terms add up to the delay of a packet sent alone, exactly: a packet with the link to
/// itself arrives when it did before occupancy was modelled.
#[test]
fn t9_serialization_and_propagation_add_up_to_the_message_delay() {
    let config = SimpleNetworkConfig::wan();
    for size in [0, 1, 1460, 1461, 65_536, MIB] {
        let channel = config.channel_config(Link::new(party(0), party(1)));
        let (serialization, propagation) = channel.transmission(size);
        assert_eq!(
            serialization + propagation,
            channel.message_delay(size),
            "size {size}"
        );
    }
    let own = config.channel_config(Link::new(party(0), party(0)));
    assert_eq!(own.transmission(MIB), (Duration::ZERO, Duration::ZERO));
}
```

The assertions are exact (`assert_eq!` on `Duration`), not approximate. They can be, because the
switchboard adds the same `Duration`s the test reads from `transmission`.

### 4.5 Step 3 — The split

In `src/net/simulation/channel.rs`, beside `message_delay` (`:253-258`):

```rust
/// Splits [`message_delay`](Self::message_delay) into the two parts of a transfer:
/// `(serialization, propagation)`.
///
/// *Serialization* is the time the link is busy pushing the message's bits out, at the
/// channel's throughput. While it lasts, the link cannot start another message. *Propagation*
/// is the one-way latency, half the RTT, which every message pays whatever else is on the
/// link. The two always add up to `message_delay(n_bytes)` exactly.
pub fn transmission(&self, n_bytes: usize) -> (Duration, Duration) {
    match self.net_type {
        NetworkType::Tcp => {
            // The serialization time is what is left of the total once the propagation time
            // is taken out, instead of being computed on its own: two separately rounded
            // terms could add up to 1 ns more or less than `message_delay`.
            let total = self.recv_time_tcp(n_bytes);
            let propagation = Duration::from_secs_f64(self.rtt.to_secs() / 2.0).min(total);
            (total - propagation, propagation)
        }
        NetworkType::Instant => (Duration::ZERO, Duration::ZERO),
    }
}
```

**Why the serialization time is a subtraction.** `recv_time_tcp` converts one `f64` to a
`Duration` (`:243-244`). Converting `bits / throughput` and `rtt / 2` separately rounds twice, and
the two can add up to 1 ns more or less than today's value. The golden test records exact
nanosecond timestamps, so every scenario would shift, and the real changes would be buried.
Taking the propagation time out of the total keeps the sum exactly equal to `message_delay`, which
`t9_serialization_and_propagation_add_up_to_the_message_delay` checks. The result in the prototype:
8 of the 11 golden scenarios did not change by a byte.

`NetworkType::Instant` returns `(0, 0)`, as the To-do amendment asks.

**Why `.min(total)`.** Subtracting `Duration`s panics when the result would be negative. In exact
arithmetic the total is never smaller than half the RTT, but both values come out of a conversion
from `f64`, and the guard costs nothing.

### 4.6 Step 4 — The trait and the switchboard

```rust
/// Computes a network delay over a link.
pub trait Delay: Send {
    /// The delay for the link.
    fn delay(&self, link: Link, size_bytes: usize) -> Duration;

    /// The two parts of the delay: `(serialization, propagation)`.
    ///
    /// The link is busy with a message for its serialization time, and a message sent meanwhile
    /// on the same link starts once the link is free. The propagation time is added after that,
    /// and does not hold the link. The default has no serialization time, so messages on one link
    /// never wait for each other.
    fn transmission(&self, link: Link, size_bytes: usize) -> (Duration, Duration) {
        (Duration::ZERO, self.delay(link, size_bytes))
    }
}
```

```rust
impl<N> Delay for ConfigDelay<N>
where
    N: NetworkConfig,
{
    fn delay(&self, link: Link, size_bytes: usize) -> Duration {
        self.0.channel_config(link).message_delay(size_bytes)
    }

    fn transmission(&self, link: Link, size_bytes: usize) -> (Duration, Duration) {
        self.0.channel_config(link).transmission(size_bytes)
    }
}
```

A field beside `last_arrivals` (`switchboard/mod.rs:161`), initialised empty in `new`:

```rust
/// The virtual instant until which each link is busy serializing the messages already sent on
/// it (see [`send`](Switchboard::send)). Per link, like `last_arrivals`: the sessions on a
/// link share its bandwidth.
busy_until: HashMap<Link, Duration>,
```

And in `send`, in place of the line that computes `arrival_time` (`:245`):

```rust
// Link occupancy: the link serializes one message at a time, so this one starts once the
// messages sent before it on this link are out, and holds the link for its own
// serialization time. The propagation time is added after that and holds nothing.
let (serialization, propagation) = self.delay.transmission(link, packet.size());
let start = now.max(self.busy_until.get(&link).copied().unwrap_or_default());
let busy_until = start + serialization;
self.busy_until.insert(link, busy_until);

// Clamp the arrival so it is not earlier than the previously scheduled arrival on this
// link (per-link FIFO); ties are broken by `seq`, which preserves the send order.
let mut arrival_time = busy_until + propagation;
if let Some(&last_arrival) = self.last_arrivals.get(&link) {
    arrival_time = arrival_time.max(last_arrival);
}
self.last_arrivals.insert(link, arrival_time);
```

- **Why a provided method on `Delay`.** Existing implementors, `ConstantDelay` and any type of a
  user's own, keep compiling and keep their behaviour (plan §5.7). With the default the
  serialization time is zero, so `busy_until` is just `now` and nothing changes.
- **Keep the FIFO clamp.** With `ConfigDelay` it is now redundant, since arrivals on a link are
  already in order. With a custom `Delay` on the default `transmission`, a small message can still
  be given a shorter delay than a large one sent before it.
- `busy_until` is per `Link`, like `last_arrivals`: the sessions on a link share its bandwidth.

### 4.7 What else changes

**The golden snapshot: 3 of 11 scenarios.** Regenerated in the prototype and compared with the
committed files:

| Scenario | Lines changed | Shift |
| --- | --- | --- |
| `bandwidth_flamegraph` | 4 of 179 | +0.4 ms |
| `secure_stats_flamegraph` | 12 of 785 | +0.4 ms |
| `secure_covariance` | 1555 of 2093 | +0.4 ms to +4.4 ms; elapsed 0.859 s → 0.863 s for every party |

Only timestamps change. No byte count and no folded tree line differs. The other 8 files are
identical. These are the diffs to review and to explain in the CHANGELOG (the phase's exit
condition); regenerate with `SCL_UPDATE_GOLDEN=1 cargo test --test golden`.

**T2 (b) fails.** `t2_elapsed_time_is_the_critical_path` (`tests/concurrent.rs:718`) asserts that
elapsed time is *exactly* hops × rounds. With occupancy, leaves that run at the same time send on
the same links, and the second packet waits for the first. The prototype's first failure: a `Par`
of two leaves took 50.01152 ms where the test expected 50.00576 ms, one serialization time
(5.76 µs) more. The model is doing what it was built to do; the property is what has to change.

A version that holds *(AI proposal; passed 800 random trees with occupancy, and passes without
it)*: the time is at least the critical path, and exceeds it by at most one serialization time per
round for every other leaf running alongside.

```rust
/// The largest number of leaves that run at the same time.
fn width(&self) -> u32 {
    match self {
        Tree::Leaf(_) => 1,
        Tree::Seq(children) => children.iter().map(Tree::width).max().unwrap_or(0),
        Tree::Par(children) => children.iter().map(Tree::width).sum(),
    }
}
```

```rust
/// T2 (b): a party's elapsed time is the tree's critical path, `Seq` adding its children's
/// rounds and `Par` taking their maximum, plus what its packets wait for the link. Leaves that
/// run at the same time send on the same links, and a link serializes one packet at a time,
/// so in each round a packet waits for at most one packet of every other such leaf.
#[test]
fn t2_elapsed_time_is_the_critical_path(tree in trees()) {
    let hop = elapsed_times(&Tree::Leaf(1))[0];
    prop_assert!(hop > Duration::ZERO);
    let (serialization, _) = SimpleNetworkConfig::wan()
        .channel_config(Link::new(party(0), party(1)))
        .transmission(32);
    prop_assert!(serialization > Duration::ZERO && serialization < hop);

    let critical_path = hop * tree.rounds();
    let waiting = serialization * (tree.width() - 1) * tree.rounds();
    for (p, time) in elapsed_times(&tree).into_iter().enumerate() {
        prop_assert!(time >= critical_path, "party {}: {:?} < {:?}", p, time, critical_path);
        prop_assert!(
            time <= critical_path + waiting,
            "party {}: {:?} > {:?} + {:?}", p, time, critical_path, waiting
        );
    }
    // Nothing runs at the same time as anything else: no packet waits.
    if tree.width() == 1 {
        prop_assert_eq!(elapsed_times(&tree)[0], critical_path);
    }
}
```

(The closing brace of the `proptest!` block is not shown.) It needs `Link` and `NetworkConfig`
imported from `simulation::channel`.

**Why this bound.** A leaf sends one packet per link in each round and then waits for that round's
replies, and a hop is longer than a serialization time (the test asserts it). So on any link, at
any instant, at most one packet of each leaf is being serialized, and a packet waits for at most
one packet of every other leaf running alongside: `width − 1` serialization times per round. The
bound is loose on purpose, because leaves fall out of step after their first wait and then stop
colliding. *(AI reasoning; supported by the 800 trees, not proved.)*

T2 (a), the 400 ms and 200 ms measurement, still passed.

**Tests with literal timestamps.** Any test that pins a rendered trace on a network with delays
can move by a millisecond. The Phase 4 rendering test did; §3.4 has the version on an
instantaneous network.

### 4.8 The benchmark

**What is needed, and what is not.** The To-do bullet asks for a new `Scenario` const, a new
protocol, a new `Which`, and `run_all.sh`. A `Scenario` is a link (RTT, bandwidth, loss, window);
the acceptance criterion compares against `BulkTransfer` "in the same regime" (R-4), which means
on an *existing* scenario. So a new protocol is enough, and no new `Scenario` is needed *(AI
reading; departs from the To-do bullet)*.

**It does not need Phase 3.** Two sends back to back on one link in one session already contend
for it, in virtual time and on a real socket. So the plan's "Phase 5 depends only on Phase 1"
holds for the benchmark too. **Why one acknowledgement, after both messages:** party 0's span
then covers both transfers on both backends, as `BulkTransfer`'s single acknowledgement covers its
one.

The protocol, in `benches/comparison/protocols.rs`:

```rust
/// The contention probe: two large messages on one link at the same instant, acknowledged once.
///
/// Party 0 sends two `payload_bytes` messages back to back and waits for one acknowledgement,
/// which party 1 sends after it has received both. The two messages share the link, so the second
/// one finishes a full serialization time after the first. A delay model that prices each message
/// as if it had the link to itself predicts the time of one transfer instead of two.
pub struct DoubleBulk {
    /// Size of each of the two bulk messages, in bytes.
    pub payload_bytes: usize,
}

impl<E: Environment> Protocol<E> for DoubleBulk {
    /// Total bytes this party received over the run.
    type Output = usize;

    async fn run(self, environment: &mut E) -> Result<usize, Error> {
        let me = environment.network().local_party().as_usize();
        let other = environment.network().other()?;

        if me == 0 {
            let mut packet = Packet::empty();
            packet.write(&vec![0u8; self.payload_bytes])?;
            environment.network_mut().send_to(other, &packet).await?;
            environment.network_mut().send_to(other, &packet).await?;
            let ack = environment.network_mut().recv_from(other).await?;
            Ok(ack.size())
        } else {
            let first = environment.network_mut().recv_from(other).await?;
            let second = environment.network_mut().recv_from(other).await?;
            let mut ack = Packet::empty();
            ack.write(&vec![0u8; ACK_BYTES])?;
            environment.network_mut().send_to(other, &ack).await?;
            Ok(first.size() + second.size())
        }
    }

    fn id(&self) -> ProtocolId {
        ProtocolId::from("DoubleBulk")
    }
}
```

The other edits, all mechanical (compiled in the prototype, `cargo bench --bench comparison
--no-run`):

- `main.rs`: a `Which::DoubleBulk` variant (`:70-75`), added to `ALL` (`:79`, now `[Which; 3]`),
  slug `double_bulk` in `name` (`:82-87`), `(scenario.bulk_payload_bytes, 1)` in `workload`
  (`:96-101`), and one arm each in `emit_simulated` (`:330-338`) and `real_elapsed` (`:420-436`).
- `run_all.sh:28`: add `double_bulk` to `PROTOCOLS`.
- `plot.py:99-106`: add the slug to `PROTOCOL_LABELS` and `PROTOCOL_ORDER`. **The To-do bullet
  does not mention this file.** The plots and the summary iterate over `PROTOCOL_ORDER`
  (`:259`, `:407`), so a protocol missing from it is left out without an error. *(Read, not run.)*
- `calibrate` reads only `bulk_transfer` rows (`main.rs:469`) and needs no change.

**The simulator's predictions**, party 0, from the prototype (`comparison sim all <protocol>`):

| Scenario | `bulk_transfer` | `double_bulk`, no occupancy | `double_bulk`, with occupancy |
| --- | --- | --- | --- |
| `bandwidth_limited` | 4.409864 s | 4.409864 s | 8.719392 s |
| `window_limited` | 1.743953 s | 1.743953 s | 3.387842 s |
| `lossy` | 6.125191 s | 6.125191 s | 12.150147 s |

The middle column is the point of the phase: today the simulator predicts that two transfers take
exactly as long as one. The `bulk_transfer` column is unchanged by occupancy and matches the
README's table (4.410, 1.744, 6.125).

**What remains for you: the real runs.** `run_all.sh` shapes the link with `tc netem`, which
needs root; it was not run. Then apply the criterion (plan §5.7), on `bandwidth_limited` and
`window_limited`:

1. the absolute relative error of `double_bulk` against the real median is no larger than
   `bulk_transfer`'s on the same scenario (today −10.6 % and +7.0 % nominal, from the README);
2. it is smaller with occupancy than without. The "without" prediction is the middle column
   above, so it can be compared by hand without a second build.

One thing to watch in the real numbers *(AI expectation, not measured)*: a real run pays TCP slow
start once per connection, not once per message, so the second transfer of a real `double_bulk`
may run faster than the first. The simulator models no slow start at all, and the README already
lists it as uncorrected.

### 4.9 If the criterion fails

C-6 then makes occupancy opt-in. *(Sketch only, not prototyped.)* `simulate` builds its delay
model itself, `ConfigDelay(config)` (`src/net/simulation/simulator.rs:466`), so there is nowhere
for a caller to choose. The smallest change is a provided method on the simulator's
`NetworkConfig` trait, for example `fn link_occupancy(&self) -> bool { false }`, read by
`ConfigDelay::transmission` to pick between the split and the default. T2 (b) could then keep its
exact form on a configuration that leaves occupancy off.

### 4.10 To-do bullets

| To-do bullet | Section |
| --- | --- |
| Skeleton | §4.3 |
| Tests: T9 | §4.4 |
| `Delay::transmission`; `ConfigDelay` split; `busy_until` | §4.5, §4.6 |
| `benches/comparison` scenario | §4.8 |
| Exit: T9; acceptance criterion; golden diffs; README figures | §4.4, §4.7, §4.8 |
| Amendment: the tolerance that does not exist | §4.8 (the criterion) |
| Amendment: `(0, 0)` for `Instant` | §4.5 |

---

## 5. Order of work

Phases 3 and 4 do not depend on each other; Phase 5 depends on neither (plan §7). An order that
keeps every commit green:

1. **The port helper** (§1). One function, no behaviour change.
2. **Phase 4** before Phase 3 *(suggestion)*. It is the smaller of the two, it touches only the
   simulator, and the golden snapshot tells you at once whether it is right.
3. **Phase 3.** Skeleton, the ten tests, then §2.5 to §2.9 in order. The tests cannot pass
   piecemeal: the old `StreamMap` code and the new mailboxes do not coexist, so expect one large
   change to `tcp.rs`.
4. **Phase 5**, for 0.15.1. In one commit: the split, the switchboard, T9, the revised T2 (b), and
   the three regenerated golden files with their explanation. The benchmark protocol can be a
   separate commit, and the real runs decide C-6.

Phases 1–4 ship together as 0.15.0 (plan §9), so nothing here needs to be released between
steps.
