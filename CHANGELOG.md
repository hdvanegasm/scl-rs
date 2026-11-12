# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
scl-rs stays on `0.x` indefinitely (there is no planned `1.0`); breaking changes may occur in any
`0.x` release and are bumped in the minor position (`0.y`).

## [Unreleased]

## [0.15.0] - 2026-11-12

Adds concurrent composition: independent sub-protocols, of the same or different types, run at the
same time within one party, so the party waits for the slowest of them instead of for all of them
in turn. The `Join` and `JoinAll` combinators of `protocol::concurrent` fork the environment into
child sessions; every message carries its session, both backends keep sessions apart, and traces
and bandwidth trees follow them. On `TcpNetwork`, each connection now has a reader and a writer
task. The simulator also models link occupancy, so messages sent at once on one link share its
bandwidth. Also new: Schnorr's interactive proof of knowledge, in `protocol::zk`. Breaking: `Event`
gains a `session` field; the `TcpNetwork` wire format changes and `create` takes a required root
tag; `GeneralEnv` is renamed `GeneralEnvironment`; the MSRV rises to 1.88; and simulated timings
change where one link carries several messages at once. *Changed* lists each break and, where code
has to change, how to migrate.

### Added

- **`protocol::zk`**, a new module for zero-knowledge proofs run as two-party `Protocol`s between
  a prover and a verifier. The verifier's output is its verdict; the prover has no output.

- **`protocol::zk::schnorr::SchnorrPok`**, Schnorr's interactive proof of knowledge of a discrete
  logarithm over any `EllipticCurve`: the prover shows it knows `x` with `h = g^x` in three moves
  (commitment `u = g^r`, challenge `c`, response `z = r + c·x`), and the verifier accepts if and
  only if `g^z = u · h^c`. It is zero-knowledge against an honest verifier only, and interactive —
  no Fiat–Shamir transform is applied.

  The prover and the verifier are named by `PartyId`, so the proof runs between any two distinct
  parties of a larger network, for example as a sub-protocol of an MPC protocol; the other parties
  do not run it and exchange no messages with it. Build the two nodes with
  `SchnorrPok::prover(prover, verifier, x, h)` and `SchnorrPok::verifier(prover, verifier, h)`;
  both return `Error::Input` when `prover == verifier`. `Output` is `Option<bool>`: `Some(true)` or
  `Some(false)` on the verifier, `None` on the prover — a rejected proof is an ordinary output, not
  an error. Running a node whose local party does not match its role fails with `Error::Input`
  before any message is sent: the local party is neither the prover nor the verifier, the prover
  lacks the witness, or the verifier was given one.

  The `schnorr_pok` example keeps its own self-contained copy of the protocol, restricted to a
  two-party network, and does not use this type.

- **`net::session::SessionId`**, the identifier of a session, for concurrent composition. The
  simulator uses it to keep sessions apart and
  to tag trace events (see *Changed*), and `ForkNetwork::fork` creates handles on child sessions
  (see below). A session is two things: a 32-byte **root tag**, which identifies one execution
  and must be equal at every party of it and distinct across executions; and a **path** of fork
  indices from that execution's root session.
  - `SessionId::root(tag)` builds the root session.
  - `with_child(index)` and `parent()` move down and up the tree; `depth()`, `root_tag()` and
    `path()` read it.
  - `digest()` returns a domain-separated SHA-256 commitment to the root tag and the path, to bind
    a session into transcripts and commitments.
  - `Display` shows the path only: `root` for the root session, otherwise the fork indices joined
    by dots, e.g. `0`, `1.0`, `0.3.1`. The root tag is not shown.

  A `TcpNetwork` frame carries only the path, since the root tag is fixed per network instance.
  `SessionId` is re-exported from the prelude.

- **`Display` for `PartyId`**, which shows the bare index, e.g. `2`. Width, fill and alignment
  apply as for an integer.

- **`net::session::derive_root_tag(context)`** derives a root tag from public context that every
  party of an execution knows: SHA-256 over a domain-separation prefix and `context`. No
  communication is involved. The tag is unique per execution only if `context` is, e.g. the
  protocol name and version, the party set, the parameters, and an epoch counter that is never
  reused.

- **`net::simulation::simulator::SIMULATION_ROOT_TAG`**, the fixed root tag of every simulated
  execution. A fixed tag is safe in the simulator: one `simulate` call is one execution, and runs
  cannot exchange messages with each other. `simulate` keeps its signature.

- **`net::ForkNetwork`**, a network handle that can be split into handles on child sessions, so
  that independent sub-protocols can run at the same time within one party without mixing their
  messages. `fork()` returns a handle on the next child session of the handle's own session, and
  `session()` returns that session. Child sessions are numbered by the order of the forks, with no
  communication: the `i`-th fork of a handle, counting from 0, is on `session().with_child(i)`, and
  a child handle starts its own count at 0. So the parties agree on a session only if they fork
  alike: within a session, every party must make the same forks in the same order. After one
  skipped fork, every later fork in that session gets a different index on the party that skipped
  it. `SimNetwork` and `TcpNetwork` implement the trait.

- **`protocol::ForkEnvironment`**, an `Environment` whose `fork()` returns a child environment on
  a forked network handle. `GeneralEnvironment` implements it when its network implements
  `ForkNetwork` and its RNG implements `SeedableRng`. The child's RNG is seeded from the parent's
  RNG, so a run with seeded RNGs stays reproducible. Forking draws from the parent's RNG, though,
  so running sub-protocols under a combinator instead of one after another changes the values they
  sample.

- **`protocol::concurrent`**, combinators that run independent sub-protocols concurrently within
  one party. Sub-protocols called one after another with `execute` make a party wait for the sum
  of their latencies; under a combinator, it waits for the slowest one.
  - `Join((p0, p1, …))` runs 2 to 6 protocols of possibly different types, and outputs the tuple
    of their outputs.
  - `JoinAll(vec![p0, p1, …])` runs any number of protocols of one type, and outputs a `Vec` of
    their outputs in input order. An empty `Vec` outputs an empty `Vec`.

  Both are ordinary `Protocol`s: they nest, and are called with `execute`. Their environment must
  implement `ForkEnvironment`. A combinator forks the environment once per sub-protocol, in order
  and before any of them starts, and runs each sub-protocol in its own child session, so messages
  of different sub-protocols never mix, even over the same links. No task or thread is spawned:
  the concurrency stays within the party's task, adds no CPU parallelism, and is deterministic on
  the simulator. `Join`, `JoinAll` and `ForkEnvironment` are re-exported from the prelude.
  - **Every party must run every `Join` and `JoinAll`, with the same number of sub-protocols**,
    including a party that takes no part in some of them; its sub-protocol there returns at once.
    A protocol that rejects non-participants cannot be that sub-protocol: `SchnorrPok`, for
    example, returns `Error::Input` on them. On the simulator, a violation ends in the deadlock
    panic; on a real network, the receives hang until they time out.
  - Combinators fail fast: they return the first error any sub-protocol returns, and drop the
    others unfinished. The messages those already sent are never received, and the matching
    sub-protocols at the other parties may wait until their own timeout or error.
  - **Fork only through the combinators.** A combinator runs nothing in its own session while its
    sub-protocols run, and simulation traces rely on that to place each sub-protocol's scope (a
    trace records no fork). Code that calls `fork` by hand must do the same: run nothing else in
    its own session until the protocols on the child have returned or been dropped. Otherwise
    outputs and timings stay correct, but the trace places the child's scope inside the wrong
    protocol, and `bandwidth_tree_for` may return `MalformedTrace`.

- **`Protocol::boxed(map)`** and **`protocol::concurrent::BoxedProtocol<E, O>`**, to run protocols
  of different types in one `JoinAll`. `boxed` erases the protocol's type and maps its output to
  `O`, for example into an enum of the caller's:
  `JoinAll(vec![a.boxed(Pre::Shares), b.boxed(Pre::Doubles)])`. The protocol and the map must be
  `'static`; the environment need not be. A boxed protocol reports the inner protocol's `id`, so
  its trace scope keeps the inner protocol's name.

- **`NetworkError::PeerFailed { peer, reason }`**: the connection with `peer` failed, through a
  read error, a frame that does not decode, or a write error; `reason` is that error's message.
  `TcpNetwork` returns it from every later send to that peer and receive from it (see *Changed*),
  so every session learns of the failure, whichever asks first. `NetworkError` is
  `#[non_exhaustive]`, so no `match` breaks.

- **`SimulationTrace::session(&session)`** returns the events of one branch: those recorded in
  `session` or in a session below it. Events that carry no session (`Start`, `Output`, `Stop`, …)
  are left out.

- **`SimulationError::MalformedTrace { event_idx, reason }`**, returned by
  `SimulationOutcome::bandwidth_tree_for` when a party's protocol scopes do not nest: a
  `ProtocolEnd` whose protocol is not the one open in its session, or that ends in a session where
  no scope is open. `SimulationError` is `#[non_exhaustive]`, so no `match` breaks.

- **`Delay::transmission(link, size_bytes)`**, which splits a message's delay into the time it
  holds the link and the time it then travels, returned as the new
  **`net::simulation::switchboard::TransmissionTime`** (`new`, `serialization()`,
  `propagation()`). It is a provided method, whose default has no serialization time.
  `ConfigDelay`, the delay model `simulate` uses, overrides it with the new
  **`ChannelConfig::transmission(n_bytes)`**, whose two parts always add up to
  `message_delay(n_bytes)` exactly. See link occupancy under *Changed*.

- **`examples/concurrent_preprocessing.rs`**: the preprocessing of a Beaver multiplication run
  one step after another and with its five independent steps in one `Join`, on the WAN preset,
  with the two runs' metrics side by side. A party other than the king takes 400 ms one after
  another and 200 ms under the `Join`, and sends the same bytes.

- **Cancel safety is documented** for every async method of `Network`, `TcpNetwork`,
  `SimNetwork`, `Protocol`, `Join` and `JoinAll`, under `# Cancel safety`: the receives and sends
  are cancel safe on both backends; `TcpNetwork::recv`, `TcpNetwork::create` and `close()` on a
  `TcpNetwork`'s root handle are not; a protocol is not, and is made safe to drop by running it in
  a forked session.

### Changed

- **Breaking: `TcpNetwork::create(id, config, root_tag)`** takes the execution's root tag as a
  third, required parameter. Every party must pass the same tag, and a tag must not be reused for
  another execution on the same parties. There is deliberately no default, because a silently
  reused tag is the failure the parameter exists to prevent. Migration: derive one with
  `derive_root_tag`, e.g.
  `TcpNetwork::create(id, config, derive_root_tag(b"my-protocol/v1 run=1"))`. The tag is the root
  of every session of the network, and is never sent: a frame carries only its session's path, and
  each party rebuilds the session from its own tag.

- **Breaking: `TcpNetwork` is session-aware, and its wire format changes.** Each frame carries the
  session it was sent in: its body is the `postcard` encoding of the session's path and the packet,
  where it was the packet alone, so a party on 0.14.x and a party on this version cannot talk to
  each other. Each connection now has a reader task and a writer task. The reader files every
  frame under its session and sender, and a receive takes only from its own session; a frame for a
  session this party has not forked yet waits until it does, and frames for a session whose handle
  was dropped or closed are discarded. For code that never forks, the following also changes:
  - `send_to` returns once the frame is queued for the peer's writer task, not once it is written.
    Dropping a send, as a fail-fast `Join` does to a sibling, can no longer leave part of a frame
    on the stream.
  - A failed connection with a peer (a read error, a frame that does not decode, or a write error)
    is reported as `NetworkError::PeerFailed` by every later send to that peer and, once the
    packets already received from it are taken, by every later receive from it, in every session.
    0.14.x returned the `IoError` or `SerializationError` itself. A clean close is still
    `ConnectionClosed(Some(peer))`.
  - `recv_any` and `recv_any_with_timeout` return the packet of the lowest ready sender, as the
    simulator does, instead of following the order of the underlying `StreamMap`. They no longer
    return a single peer's error: they wait while any peer is open, and return
    `ConnectionClosed(None)` once none is and nothing is left to take in the session.
  - `party_ids()` returns the parties in ascending order, instead of `HashMap` order.
  - After `close()` on the root handle, receives from every party, this node included, in every
    session, still take what is already queued, then end with `ConnectionClosed(Some(party))`,
    where a `recv_from` returned `PartyNotFound`; sends to every party, this node included, fail
    with `ConnectionClosed(Some(party))`, where a send to this node returned `SendError`.
    `close()` on a forked handle gives up only that handle's session.
  - `NetworkError::SendError` is no longer returned: it came only from the old loop-back channel.

- **Breaking: `protocol::GeneralEnv` is renamed `GeneralEnvironment`**, in `protocol` and in the
  prelude. Its type parameters, fields, `new` and trait impls are unchanged. Migration: rename the
  imports and uses, e.g. `GeneralEnvironment::new(network, rng)`.

- **MSRV raised from 1.85.1 to 1.88**, because the new `sha2` 0.11.1 dependency (used by
  `SessionId::digest`) requires it. The CI MSRV job builds on 1.88.

- **Breaking: `Event::{SendData, ReceiveData, ProtocolBegin, ProtocolEnd}` gain a
  `session: SessionId` field**: the session the packet was sent or received in, or the protocol
  ran in. In code that never forks, it is always the root session,
  `SessionId::root(SIMULATION_ROOT_TAG)`; under `Join` or `JoinAll`, each sub-protocol's events
  carry its own child session. The other variants and the `Display` of events are unchanged; the
  `Display` of a trace changes only for runs that fork (see below). Migration: patterns that list
  every field need `session` or `..`; struct
  literals, e.g. in a hand-built trace passed to `SimulationTrace::new`, need
  `session: SessionId::root(SIMULATION_ROOT_TAG)`.

- **The simulator keeps sessions apart.** Its mailboxes, waiting receives and scheduled
  deliveries are keyed by link and session, so a receive takes only packets sent in its own
  session, and a delivery wakes only receives waiting in that session. Delivery on a link stays
  first-in, first-out across sessions, as on one TCP stream. For code that does not fork sessions,
  this routing changes nothing: with it alone, the golden snapshot in `tests/golden.rs` (traces,
  exact event timestamps, bandwidth trees and metrics) was unchanged. Link occupancy (below) does
  change some timings.

- **Simulated links now carry one message at a time (link occupancy).** A message holds its link
  for its serialization time (its size over the channel's throughput), and a message sent on the
  same link meanwhile starts once the link is free; the propagation time, half the RTT, is added
  after and holds nothing. Before, every message got the link's whole bandwidth, so messages sent
  at once on one link arrived together, which underestimated latency whenever a link carried more
  than one message at a time, as concurrent sessions do. A message sent alone arrives exactly when
  it did before. Messages on different links still do not share bandwidth (an uplink shared across
  links is not modelled). Occupancy applies to every simulation: `simulate` always uses
  `ConfigDelay`, which splits each channel's delay with `ChannelConfig::transmission`.

  Timings change for code that sends several messages on one link at the same instant. In the
  golden snapshot, 2 receives in `bandwidth_flamegraph` and 8 in `secure_stats_flamegraph` arrive
  0.4 ms later; `secure_covariance`, which sends many packets on one link at once, finishes at
  0.863 s instead of 0.859 s, with receives up to 4.4 ms later. The order of events, the bandwidth
  trees and the byte counts are unchanged. Occupancy has not been checked against real concurrent
  transfers: the `benches/comparison` scenarios keep one message in flight.

- **Traces and bandwidth trees follow sessions.** Both are now read off one tree of protocol
  scopes, built with a stack of open scopes per session, so the interleaved events of a `Join`'s
  branches are attributed to the branch that recorded them.
  - `bandwidth_tree_for` puts each branch under the `Join` or `JoinAll` node with its own bytes,
    e.g. `<simulation>;Preprocessing;Join;PassiveRandShr`. A branch that a fail-fast combinator dropped
    is closed with the combinator. A `ProtocolEnd` that does not match the scope open in its
    session is now an error, `MalformedTrace`, where the old parser closed the innermost scope.
  - A trace's `Display` prints each branch as its own block, labelled with its session, e.g.
    `PassiveRandShr [session 0.2] {`; a scope that never ended closes with `} (not finished)`; an
    unmatched `ProtocolEnd` prints as a plain event line instead of failing. A trace of code that
    never forks renders exactly as before.

- **`TcpNetwork::send` returns the total payload bytes it sent**: one packet's size per party,
  this node included. 0.14.x returned the size of the packet sent to the last party, i.e. one
  packet's size.

- **A deadlock in `simulate` now says what is stuck.** The panic message keeps its first line,
  `scheduler: N task(s) parked, nothing to deliver (deadlock)`, and adds a report: each receive
  still waiting, and each packet that reached its recipient but was never received, together
  with what that recipient is doing. For example, a recipient waiting in the same session for a
  different sender usually means the protocol receives from the wrong party. The report is
  sorted, so it is the same in every run. Tests that match the first line with
  `#[should_panic(expected = …)]` still pass. `simulate` documents the panic under `# Panics`.

## [0.14.2] - 2026-09-21

Adds the flat, whole-run summary of what a simulation cost. `SimulationOutcome::metrics` folds
every party's trace into two numbers — payload bytes sent, and virtual time elapsed — and prints
them as a small table: the summary counterpart to the per-protocol breakdown `bandwidth_tree_for`
already gave. Both figures are read back out of the recorded events, so nothing needs instrumenting
and no run behaves differently. The rest is dependency hygiene: every requirement in `Cargo.toml`
now names the full patch version it is tested against rather than a bare major, and `rustls` and
`chacha20` — already pulled in transitively by `tokio-rustls` and `rand` — are declared directly to
hold their floors there too.

### Added

- **`SimulationOutcome::metrics`** returns a `SimulationMetrics`: the payload bytes each party put
  on the wire over the whole run, and the virtual time each party spent in it. Read one party's
  figures with `SimulationMetrics::sent_bytes_for` and `SimulationMetrics::elapsed_time_for` — both
  return `None` for exactly the parties that took no part in the run — or print the whole set with
  its `Display`:

  ```text
  simulation metrics:
                      sent  elapsed
      party 0      1.5 MiB   1.250s
      party 1      1.5 MiB   1.250s
      party 2    768.0 KiB   0.750s
      whole run    3.8 MiB   1.250s
  ```

  One row per party in `PartyId` order — id order rather than hash order, so the output is
  reproducible across runs like the rest of the simulator — then a row for the run as a whole. Byte
  counts are IEC binary units (`KiB`, `MiB`, …) at one decimal place, via the new `bytesize`
  dependency; times are seconds to three decimals, matching an `Event` timestamp. The summary
  carries no trailing newline, so it drops into a larger report.

  The two columns aggregate differently on the `whole run` row, because they are different kinds of
  quantity: bytes add up over the parties, while their elapsed times overlap, so the run lasts as
  long as its slowest party rather than as long as all of them put together.

  Bytes are counted exactly as `bandwidth_tree_for` and `MetricHook` count them — *payload* sizes
  rather than wire bytes, so the framing and TLS overhead a real `TcpNetwork` deployment adds is
  not included, and a party's sends to itself are excluded because `TcpNetwork` loops those back
  in-process rather than over a socket. A party's total therefore agrees with the sum over its
  bandwidth tree.

  Elapsed time is *virtual* time on the simulator's discrete-event clock, spanning a party's first
  recorded event to its last. It measures the modelled network — link latency and bandwidth, as
  configured by the `NetworkConfig` the run was given — and nothing else, since local computation
  is free in this model; it is a communication cost, not a running time comparable against a real
  deployment. One consequence is worth knowing before reading the column: a party's clock advances
  only when a message is delivered *to* it, so a party that only ever sends reports
  `Duration::ZERO` however much traffic it pushed, and any party's figure really reports when its
  last message arrived.

  `SimulationMetrics::empty` builds the set that records no parties at all; it is what `metrics`
  folds the traces into, and on its own it renders as `simulation metrics: <none>`.

- **Every simulator example now prints its run's metrics**, next to the traces it already printed:
  `simple_send_recv`, `send_different_types`, `additive_shr_secure_sum`, `schnorr_pok`,
  `bandwidth_flamegraph`, `secure_stats_flamegraph` and `secure_covariance`. In the three
  flamegraph examples it is the flat view of the same traffic the graph breaks down per protocol.

### Changed

- **Dependency requirements name the full patch version they are tested against.** `tokio = "1"`
  becomes `tokio = "1.53.1"`, `thiserror = "2.0"` becomes `"2.0.20"`, and so on across the
  manifest. The requirements are still caret requirements, so the versions a consumer resolves are
  unchanged; what changes is the floor, which now states the minimum each dependency is actually
  built and tested against rather than a major-version-wide claim that was never checked.

- **`rustls` and `chacha20` are declared directly, to hold their floors.** Neither is used by name
  in the source: `rustls` is reached through `tokio-rustls` and `chacha20` through `rand` 0.10's
  default RNG. Declaring them pins those transitive versions at `0.23.45` and `0.10.2`
  respectively. Both sit in `[dependencies]`, so the floors bind a downstream consumer's
  resolution too, not just this repository's lockfile and CI.

- **Internal: `src/math/ring.rs` moved to `src/math/ring/mod.rs`.** A file move with no content
  change; the `math::ring` module path is unaffected.

## [0.14.1] - 2026-08-24

Widens `Vector` and `Matrix` past ring elements. Their operators were bounded on `Ring`; they are
now bounded on the element operation itself, so a container may hold ring elements *or* secret
shares, and `+`, `-`, `+=`, `-=` and unary `-` over a container of shares are the local,
communication-free operations of a linear scheme — a party computes on a whole vector or matrix of
shares without a round of communication. Scaling a container of shares by a public constant is the
new `mul_constant` on both types. Share × share multiplication stays out of reach, since it needs
an interactive protocol, so the element-wise and matrix products remain ring-only. Also fixes
scalar multiplication of a non-square `Matrix`. Nothing here is breaking: `Ring` implies every new
bound, so `0.14.0` code compiles unchanged.

### Added

- **Scalar multiplication for `Vector`.** `Vector<T> * &T` and `&Vector<T> * &T` scale every entry,
  mirroring the `Matrix<T> * &T` operator that already existed.

- **Negation for `Matrix`.** `-Matrix<T>` and `-&Matrix<T>`, bounded on
  `T: Neg<Output = T> + Clone`, closing a long-standing asymmetry with `Vector`, which has had
  `Neg` throughout. Works over ring elements and over shares.

- **`Vector::mul_constant` and `Matrix::mul_constant`** scale a container of *shares* by a public
  constant — the `(i, j)` output share is `[c · x_ij]`, computed locally with no communication (see
  `LinearShare`).

  These are inherent methods rather than `Mul` operators because the scalar lives in `S::Value`
  while the entries are shares, and the coherence checker rejects the resulting impl: nothing
  prevents `S::Value` from being `Vector<S>`, so `impl<S: LinearShare> Mul<&S::Value> for Vector<S>`
  overlaps the element-wise `Mul<&Vector<S>>`. This follows the convention already set by
  `Vector::add_shares` and `Matrix::mul_shares`.

### Changed

- **`Vector` and `Matrix` operators are no longer restricted to ring elements.** `Add`, `Sub`,
  `AddAssign`, `SubAssign` and `Mul` were bounded on `T: Ring`, which reduced to `Copy` plus the
  operator used in the body. They are now bounded on the element operation itself —
  `T: for<'a> Add<&'a T, Output = T>` and so on — and read their operands by move or by `Clone`
  rather than through `Copy`, so both containers can hold values that are `Clone` but not `Copy`.

  The point is that `Vector<S>` and `Matrix<S>` now work for any `S: LinearShare`: adding,
  subtracting and negating containers of shares are the local, communication-free operations of a
  linear scheme, so a party can compute on a whole vector or matrix of shares without a round of
  communication.

  Multiplication deliberately stays out of reach for shares. Element-wise `Vector` `Mul`,
  matrix–matrix and matrix–vector products all require `T: Mul<&T>` — share × share — which is not
  a local operation and is absent from `LinearShare` by design; it needs an interactive protocol
  such as Beaver multiplication. Scaling a container of shares by a *public* constant is local, and
  is what `mul_constant` provides.

- **`Matrix`'s `Mul` impls are bounded on the operations rather than on `Ring`**, using
  higher-ranked bounds (`T: for<'a> Mul<&'a T, Output = T> + for<'a> Add<&'a T, Output = T>`). The
  accumulator in matrix–matrix and matrix–vector products is now seeded with the first term of each
  sum rather than a zero element, so no additive-identity bound is needed; `Matrix::allocate`
  rejects zero dimensions, so that term always exists.

### Fixed

- **Scalar multiplication of a non-square matrix was wrong.** `Matrix<T> * &T` iterated its inner
  loop over `0..self.rows` where it should have used `0..self.columns`. A matrix with more rows than
  columns panicked through the `expect` on `get_mut`; one with more columns than rows silently left
  the trailing columns unscaled. Square matrices — all the operator had test coverage for — were
  unaffected.

## [0.14.0] - 2026-08-23

Rebuilds the finite fields on `Fp`, a prime field generic over a modulus fixed at compile time,
with Montgomery arithmetic. `Mersenne61`, `Secp256k1PrimeField` and `Secp256k1ScalarField` become
aliases for it, which deletes 352 lines of hand-written modular arithmetic and makes multiplication
~4.5× faster on the secp256k1 fields. Declaring a further field is now one macro call. Breaking:
elements serialize their Montgomery representation at fixed width, so the encoding is not
compatible with `0.13.0`.

### Added

- **`Fp<M, LIMBS>`, a generic prime field** (`math::field::fp`). The modulus is carried in the
  type as a `crypto-bigint` `ConstMontyParams` implementation, so declaring a new field no longer
  means writing another `Ring` + `FiniteField` implementation by hand:

  ```rust
  use crypto_bigint::{const_monty_params, U64};
  use scl_rs::math::{field::fp::Fp, ring::Ring};

  const_monty_params!(Mod61, U64, "1FFFFFFFFFFFFFFF", "The Mersenne prime 2^61 - 1.");
  type F = Fp<Mod61, { U64::LIMBS }>;

  let two = F::ONE + &F::ONE;
  ```

  Arithmetic is performed in Montgomery form. `Fp::new` and `Fp::retrieve` convert between an
  ordinary integer and that representation.

- Division is now covered by property tests for every field.

### Changed

- **BREAKING: field elements have a new serialized encoding.** `Mersenne61`,
  `Secp256k1PrimeField` and `Secp256k1ScalarField` are now type aliases for `Fp` and serialize
  their *Montgomery* representation at fixed width. The bytes are not interchangeable with
  `0.13.0`, on the wire or on disk, so every party in a deployment must run the same version.

  `Mersenne61` changes most visibly. It used to serialize as a `u64`, which postcard encodes as a
  varint — 1 byte for `1`, 2 for `300`, 6 for `2^40` — so a share's length on the wire correlated
  with the magnitude of the value it carried. It is now a constant 9 bytes.

  This makes simulated bandwidth **reproducible**, which it previously was not: five runs of
  `examples/secure_covariance` used to report 3634, 3640, 3632, 3637 and 3644 bytes, because the
  totals moved with whichever values the run happened to sample. The same example now reports
  3648 bytes every time. Figures quoted in `README.md` and in the example docs were updated
  accordingly.

- Deserialization now rejects out-of-range values. `ConstMontyForm`'s `Deserialize` refuses any
  encoding whose value is `>= modulus`; the previous derived implementations accepted such input
  and carried it into arithmetic that assumes a reduced operand.

- **`Ring::BIT_SIZE` is now specified as the width of the representation** — the number of bits an
  element occupies when it is stored or sent — rather than the width of the largest value. It is a
  whole number of limbs, so `Mersenne61::BIT_SIZE` is `64` rather than `61`.

### Fixed

- **`Debug` on a field element printed its Montgomery form.** The derived implementation exposed
  the internal representation, which differs from the value by a factor of $R$ — `Mersenne61::ONE`
  displayed as `0x8`, not `1`. `Fp` now formats the canonical representative, so a field element
  debug-prints the number it denotes.

- **`Mersenne61` divided in the wrong direction.** `a / b` computed `b / a`, and the error case
  was inverted along with it: `0 / x` failed with `ZeroInverse` while `x / 0` succeeded. The
  implementation inverted `self` where it should have inverted `rhs`. No other field was
  affected, and no code in the crate used the operator — `Lagrange` interpolation divides through
  `inverse()` directly — so protocol results were never wrong.

## [0.13.0] - 2026-07-22

Drops the `async-trait` dependency. The crate now uses native `async fn` in traits, stable since
Rust 1.75 and well within the crate's `1.85.1` MSRV, which is unchanged.

### Changed

- **BREAKING: `Protocol` and `Network` no longer use `#[async_trait]`.** Both traits declare their
  async methods natively. Implementors must **delete the `#[async_trait]` / `#[async_trait::async_trait]`
  attribute** from every `impl Protocol for …` and `impl Network for …` block, and may drop
  `async-trait` from their own `Cargo.toml` if it was pulled in only for this. The method bodies
  themselves do not change — they stay plain `async fn run(self, env: &mut E) -> …`:

  ```rust
  // before
  #[async_trait]
  impl<E: Environment> Protocol<E> for MyProtocol {
      type Output = usize;
      async fn run(self, env: &mut E) -> Result<usize, Error> { /* … */ }
      fn id(&self) -> ProtocolId { "MyProtocol".into() }
  }

  // after — the attribute is the only line that goes away
  impl<E: Environment> Protocol<E> for MyProtocol {
      type Output = usize;
      async fn run(self, env: &mut E) -> Result<usize, Error> { /* … */ }
      fn id(&self) -> ProtocolId { "MyProtocol".into() }
  }
  ```

  In the trait _definitions_ the async methods are spelled
  `fn … -> impl Future<Output = …> + Send` rather than `async fn`. That is deliberate and not
  cosmetic: a bare `async fn` in a public trait leaves the returned future's auto traits
  unspecified, so an implementor could return a non-`Send` future and callers driving a protocol on
  a multi-threaded runtime would fail to compile. The explicit `+ Send` preserves exactly the
  guarantee `#[async_trait]` used to provide (see the `Network: Send` note in 0.4.0). Callers and
  implementors are unaffected by this spelling.

  The one capability given up is `dyn` compatibility: `Box<dyn Protocol<…>>` and `Box<dyn Network>`
  are no longer possible. Neither was used anywhere in the crate, its tests, or its examples, and
  boxing a protocol's _future_ (as the simulator's executor does) is unaffected.
- Removed `async-trait` from both `[dependencies]` and `[dev-dependencies]`. Each async trait-method
  call no longer heap-allocates a `Pin<Box<dyn Future>>`; the effect on end-to-end protocol timings
  is below measurement noise next to field arithmetic, serialization and TLS, so no benchmark
  numbers in the README change.

### Fixed

- A needless `return` in `TcpNetwork::recv_from_with_timeout` and an unused `mut` on
  `PassiveShamirOpenKing::run`'s receiver. Both were pre-existing and invisible to clippy, which
  skips macro-expanded code — `#[async_trait]` had been suppressing lints inside every trait method
  body in the crate.

## [0.12.1] - 2026-07-21

Internal only — no API or behaviour change, and no change to any simulated timing.

### Changed

- **Expanded the simulator's internal documentation** — the executor's waker plumbing, the
  ready-queue loop and its deadlock panic, and `Switchboard::deliver_next` are now documented at
  the level the rest of `net::simulation` already was.
- Renamed three crate-private items in `net::simulation` to say what they do:
  `executor::run_with_idle` → `executor::run_simulation_with_idle`, `executor::Idle` →
  `executor::IdleOutcome`, and `simulator::drive` → `simulator::drive_party_to_completion`. All
  three are `pub(crate)`; the public API is untouched.

### Fixed

- **A self-deadlock in the simulator's executor loop, introduced on `main` after `0.12.0` and never
  present in a release.** While documenting the loop, the ready-queue pop was folded into the
  `match` scrutinee (`match ready_queue.lock()…pop_front() { … }`). A temporary in a `match`
  scrutinee lives until the end of the whole `match`, so the `MutexGuard` stayed alive across every
  arm — including `None => on_idle()`, which delivers the next event and calls `Waker::wake`, and
  `wake` re-locks that same queue. On the simulator's single thread that is an unconditional
  deadlock the first time any party parks on a receive. Restoring the `let next_task = …;` binding
  drops the guard at the end of the statement, before the `match` runs. **Users of released
  versions were never affected**; only builds tracking `main` between the two commits could hang.

## [0.12.0] - 2026-07-20

### Added

- **LAN and WAN presets for `SimpleNetworkConfig`.** `SimpleNetworkConfig::lan()` models a
  1 Gbps, 1 ms-RTT link and `SimpleNetworkConfig::wan()` a 100 Mbps, 100 ms-RTT link; both are
  loss-less and keep the 1460-byte MSS. `SimpleNetworkConfig::from_channel_config` applies an
  arbitrary `ChannelConfig` to every inter-party link. Self-links stay instantaneous in all cases.
  Each preset sets its window size at or above the link's bandwidth-delay product (128 KiB for the
  LAN, 1.25 MB for the WAN), so the nominal bandwidth is what sets the rate — at the 64 KiB default
  window the WAN link would be window-bound to ~5.2 Mbps instead of its nominal 100 Mbps. See
  `WindowSize` for why that number must be calibrated by measurement rather than read off a socket
  buffer setting.
- **Simulated-vs-real benchmark harness (`benches/comparison`).** A `tc netem`-shaped,
  mutually-authenticated-TLS comparison across the three regimes the crate models
  (bandwidth-limited, window-limited, lossy), each run 50 times against a round-dominated
  (`PingPong`) and a bandwidth-dominated (`BulkTransfer`) protocol, plus a Python plotting script
  that renders one figure per scenario and a summary table. It supersedes the ad-hoc figures
  previously quoted in the README's "Benchmarks" section; see
  [`benches/comparison/README.md`](benches/comparison/README.md) for the method and results. Two
  findings refine the earlier text: the loss-formula error is confined to bandwidth-dominated
  protocols (a round-dominated protocol under 1% loss is predicted within 0.7%, while a
  bandwidth-dominated one on the same link is over-predicted by ~400%), and the bandwidth-limited
  regime under-predicts by 3–11% because each fresh connection pays TCP slow start, which the
  simulator's steady-state model does not charge.

### Changed

- **BREAKING: `SimpleNetworkConfig` is no longer a unit struct.** It now carries the `ChannelConfig`
  it applies, so that the presets above can exist. Replace the bare value `SimpleNetworkConfig` with
  `SimpleNetworkConfig::default()`, which reproduces the previous behaviour exactly (the
  `ChannelConfigBuilder` defaults on every inter-party link, instantaneous self-links). No simulated
  timing changes for existing code.

### Fixed

- Corrected the README and crate-doc claim that "`SimpleNetworkConfig` uses instantaneous channels".
  Only a party's link to *itself* has ever been instantaneous; inter-party links have always used the
  default TCP parameters (1 Mbps, 100 ms RTT).

## [0.11.2] - 2026-07-15

### Fixed

- **The simulator's network timing model charged a full RTT of propagation per message, where a
  message only crosses the wire one way.** `ChannelConfig::message_delay` returns the one-way time
  until the recipient receives a message, but `recv_time_tcp` added a whole `rtt` (a round trip) to
  the serialization time instead of `rtt / 2` (a single one-way hop). The serialization term and the
  steady-state throughput formulas it builds on (window/RTT for the loss-less case, the Mathis
  `√(3/2p)·MSS/RTT` for the lossy case) were already correct — only the propagation term was doubled.
  A full RTT is a *round* trip, i.e. two one-way messages, so a request/response exchange was billed
  two RTTs where the physics spends one. Validated against a real mutually-authenticated-TLS run over
  a `tc netem`-shaped loopback link (1 Mbit/s, 100 ms RTT): a 20-round, 20 KB ping-pong measured
  ~8.51 s, against 10.58 s simulated before the fix (+24%) and 8.58 s after (+0.8%). **This shifts
  every simulated timing** — all propagation terms halve — so figures quoted from earlier simulator
  runs will differ; it does not affect the real `TcpNetwork` backend, which never used this model.

## [0.11.1] - 2026-07-14

Documentation only — no API or behaviour change.

### Added

- **A bandwidth-profiling walkthrough**, in the README (`## Bandwidth profiling`) and in the crate
  docs, built around a rendered flamegraph of `examples/secure_covariance.rs`
  (`docs/cov_bandwidth.svg`). The post-hoc profiler shipped back in `0.10.0`, but nothing showed what
  it is *for*: the section explains that the simulator needs no instrumentation (it already records
  every send and every `Protocol::execute` boundary, so `bandwidth_tree_for` reconstructs the call
  tree afterwards), that flamegraph **width is bytes, not time**, and reads the DN07 example's graph
  as a cost breakdown — 13% sharing the inputs, 57% preprocessing, 25% the online multiplication, 5%
  revealing the result.

### Fixed

- Corrected an overstated claim in the `secure_covariance` example, the `0.11.0` changelog entry and
  the roadmap: they described the preprocessing phase as *dwarfing* the online multiplication, which
  the measured flamegraph does not support (57% against 25% — a factor of ~2.3, and at `n = 5` the
  online phase is not remotely negligible). The prose now gives the measured figures and makes the
  claim that actually holds: most of the traffic is **input-independent**, so it can be generated
  before the data exists and lifted off the critical path.

## [0.11.0] - 2026-07-14

### Changed

- **Breaking:** `LinearShare` dealing is now threshold- and RNG-aware. The trait gains an
  associated type `Threshold` — the scheme's reconstruction-threshold parameter — and
  `shares_from_secret` now takes `(secret, parties, threshold, rng)` and returns
  `Result<Vec<Self>, ShareError>`. `ShamirSS` uses `Threshold = usize` (the polynomial degree: any
  `degree + 1` shares reconstruct) instead of hardcoding the full-threshold degree `n − 1`;
  `AdditiveSS` uses `Threshold = ()`, since additive reconstruction structurally requires every
  share and there is nothing for the caller to choose. Invalid parameterizations (a Shamir degree
  that not even all `n` shares could reconstruct) are rejected at dealing time with the new
  `ShareError::InvalidThreshold` instead of silently producing an unreconstructable sharing.
- **Breaking:** `PassiveDealShr::dealer` takes the scheme's threshold as a fourth argument, and the
  protocol's environment bound is now `RandEnvironment`: dealing draws its randomness from the
  environment's session RNG instead of `rand::rng()`, so simulator runs with seeded per-party RNGs
  are reproducible end to end.
- **Breaking:** `Ring` now requires `Neg<Output = Self>` as a supertrait, so ring elements support
  the `-x` operator and not only the `negate()` method. The three built-in field types implement it;
  out-of-tree `Ring` implementors must add an `impl Neg`, which can simply forward to `negate()`.
- `Matrix`'s `is_square`, `get`, `get_mut` and `Vector`'s `len`, `is_empty`, `Index`, `IndexMut` no
  longer require `T: Ring`, so both types can hold elements that are not ring elements — secret
  shares, in particular. Purely a relaxation; existing code keeps compiling.
- `postcard` is now depended on with `default-features = false`. Its default `heapless-cas` feature
  pulled in `heapless` and, transitively, `spin` — none of which the crate uses, since serialization
  goes through `to_allocvec`/`from_bytes` (the `alloc` feature). This drops 10 transitive
  dependencies and clears a `cargo audit` warning about the yanked `spin 0.9.8`. No API change.

### Added

- **`RandEnvironment`** — an `Environment` that additionally carries the session's
  cryptographically secure RNG, implemented by `GeneralEnv` (which now holds an `rng` alongside the
  network) and re-exported from the `prelude`. Protocols that sample secret material bound on it;
  protocols that never sample keep the plain `Environment` bound.
- **`protocol::passive_shamir`** — the passive (semi-honest) protocols of Damgård and Nielsen,
  *Scalable and Unconditionally Secure Multiparty Computation* (CRYPTO 2007), over Shamir sharing.
  All of them assume every party follows the protocol, and — except for `PassiveRandShr` — require
  `n >= 2t + 1`:
  - `PassiveRandShr` (`Random`) — `n - t` degree-`t` sharings of secrets **no party knows**. Every
    party deals one sharing of a value it samples itself, and the `n` dealt sharings are compressed
    by a Vandermonde extraction matrix into `n - t` that are uniformly random even if `t` of the
    dealers colluded in choosing their inputs. This is what makes randomness generation cost `O(n)`
    work per party rather than `O(n²)`.
  - `PassiveRandDoubleShr` (`Double-Random`) — the same, but each secret is shared twice, at degree
    `t` and degree `2t`, yielding `n - t` `DoubleShare`s.
  - `PassiveOpenToKing` and `BatchedPassiveOpenToKing` (`Open`) — reconstruction through a
    designated *king*, who collects the shares, interpolates and sends the result back: two rounds
    and `O(n)` messages instead of the `O(n²)` of an all-to-all open. The batched form opens a whole
    vector of secrets in those same two rounds, so the round count does not grow with the batch.
  - `PassiveTriple` — multiplication triples `([a], [b], [a · b])`, all at degree `t`, built from
    the above. Each triple's masked product is opened, but all of them in a **single** batched
    round.
  - `PassiveShamirMul` — Beaver multiplication: spends one triple per product to multiply live
    sharings, `[x · y] = [a · b] + d · [b] + e · [a] + d · e`, where only the masked `d = x − a` and
    `e = y − b` are opened. The result comes back at degree `t`, so it can feed the next
    multiplication. A whole batch of products costs **one** round, so a circuit's round count tracks
    its multiplicative *depth*, not its number of multiplication gates.
- **`ShamirSS: Mul<&Self>`** and **`DoubleShare`** — the local product of two Shamir shares, whose
  degrees add (`[x]_d · [y]_e = [x · y]_(d+e)`), and the correlated randomness needed to bring such
  a product back down to degree `t`. A `DoubleShare` pairs a degree-`t` with a degree-`2t` sharing
  of the same secret; it is deliberately **not** `Clone`, because reusing one across two
  multiplications would mask both products with the same value and leak them.
- **`Matrix::transpose`**, **`Matrix::vandermonde`**, **`Matrix::ones`**, **`Matrix::set`** and
  **`Matrix::mul_shares`** — `mul_shares` applies a matrix of public constants to a vector of
  secret shares, computing each output as a local (communication-free) linear combination of the
  inputs. It is an inherent method rather than a `Mul` impl, which would overlap with the existing
  `Mul<&Vector<T>>` under Rust's coherence rules. `Matrix::Error` gains an `IndexOutOfBounds`
  variant.
- **`Vector::add_shares`** — adds a vector of public constants to a vector of shares element-wise,
  the counterpart of `Matrix::mul_shares` — plus `IntoIterator` for `Vector<T>` and `&Vector<T>`,
  and `Neg` for both (which now needs only `T: Neg + Clone`, so it applies to vectors of shares).
- An `examples/secure_covariance.rs` example: the **covariance between two parties' private
  datasets**, the first example with real secure multiplication. Unlike the variance in
  `secure_stats_flamegraph.rs` — where each party can square its own input locally, dodging
  multiplication entirely — covariance multiplies one party's `xᵢ` by *another* party's `yᵢ`, so no
  party can compute the product on its own and DN07's interactive multiplication is unavoidable.
  The circuit centres both vectors on their (secret) means for free, since Shamir sharing is linear,
  and multiplies all `ℓ` pairs in a single round. The example splits preprocessing from the online
  phase and exports a bandwidth flamegraph attributing 57% of the traffic to triple generation
  against 25% for the multiplication that spends it — the majority of the bandwidth depends on no
  input at all and can be lifted off the critical path. Run with
  `cargo run --example secure_covariance`.

## [0.10.0] - 2026-07-10

### Added

- **`MetricHook`** — the first built-in `TriggeredHook`: it reacts to `SendData` events and totals
  the payload **bytes** each party puts on the wire, both in aggregate (`total_data`) and per sending
  party (`total_data_by`). Counters are handed in as `Arc<Mutex<_>>` so the caller keeps a view of
  them after the hook has been moved into `simulate`; register a clone of the `Arc<MetricHook>` and
  read the totals back once the run has finished. Note the counters measure bytes, not message
  counts, and that sends from a party to itself are excluded — they never touch the wire, since
  `TcpNetwork` delivers them over an in-process loop-back channel.
- **`ProtocolId`** — a `Copy`, allocation-free newtype over `&'static str` naming a protocol, built
  with `ProtocolId::from("…")` and rendered through `Display`. Re-exported from the `prelude`
  alongside `Protocol`, so implementing the trait no longer needs a second import.
- **Post-hoc per-protocol bandwidth profiling** — `SimulationOutcome::bandwidth_tree_for(party)`
  reconstructs a `ProtocolBandwidthTree` from the party's recorded trace: the call tree of
  (sub-)protocol invocations, with every sent byte attributed to the innermost protocol running
  when it was sent. Protocols need no instrumentation — the tree is rebuilt after the run from the
  `ProtocolBegin`/`ProtocolEnd`/`SendData` events the simulator records anyway. The root is a
  synthetic `<simulation>` node spanning the whole run (bytes sent outside any protocol scope are
  attributed to it), sizes are payload bytes, and self-sends are excluded — the same accounting as
  `MetricHook`. Asking for a party that was not simulated returns
  `SimulationError::PartyNotFound`.
- **`ProtocolBandwidthTree::write_folded(&mut impl io::Write)`** — serializes the bandwidth tree
  in the folded-stacks format of Brendan Gregg's flamegraph tooling
  (<https://www.brendangregg.com/flamegraphs.html>): one line per call path that sent bytes
  itself, e.g. `<simulation>;SecSumShamirShr;InputPhase;PassiveDealLinearShr 20`. Render with
  `inferno-flamegraph --countname bytes` or `flamegraph.pl --countname=bytes`; line values are
  self bytes (renderers sum descendants into ancestors themselves), and concatenating several
  parties' trees into one file is valid — renderers sum duplicate paths into network-wide totals.
- An `examples/bandwidth_flamegraph.rs` example: a three-party Shamir secure summation composed of
  nested protocols, profiled post-hoc and exported to `bandwidth.folded`, together with the
  `inferno` command that renders the SVG. Run with `cargo run --example bandwidth_flamegraph`.
- An `examples/secure_stats_flamegraph.rs` example: secure **mean and variance** among five
  parties, one composition level deeper, so the flamegraph shows two distinct towers
  (`SumValues` / `SumSquares`, each over an `InputPhase` of Shamir deals plus an open). Variance
  needs no secure multiplication: each party shares its own square, keeping the whole computation
  linear, and the mean/variance arithmetic finishes in the clear on the two opened sums. Also
  demonstrates giving repeated phases **distinct `ProtocolId`s** so renderers don't merge their
  towers. Run with `cargo run --example secure_stats_flamegraph`.

### Changed

- **Breaking: `Protocol::name` is renamed to `Protocol::id` and now returns `ProtocolId`** instead of
  `&'static str`. Every `impl Protocol for …` must be updated:

  ```diff
  - fn name(&self) -> &'static str { "MyProtocol" }
  + fn id(&self) -> ProtocolId { ProtocolId::from("MyProtocol") }
  ```

  The rename separates a protocol's *trace identity* from any human-readable description, and the
  newtype keeps the tracing path in `Protocol::execute` free of allocation while leaving room for the
  id to gain structure later. `Network::record_protocol_begin`/`record_protocol_end` and the
  `Event::ProtocolBegin`/`ProtocolEnd` `protocol_name` field carry `ProtocolId` for the same reason.
- **Breaking: `TriggeredHook` moved from `net::simulation::switchboard` to a new
  `net::simulation::hook` module**, which now houses the hook extension point and its built-in
  implementations. Update `use scl_rs::net::simulation::switchboard::TriggeredHook` to
  `use scl_rs::net::simulation::hook::TriggeredHook`. The trait itself is unchanged, and the
  switchboard still dispatches the hooks; only the path moved.

## [0.9.1] - 2026-07-08

### Fixed

- **`recv_any_with_timeout` no longer leaves spurious wakers parked after a timeout.** When the
  any-party receive resolved to `Timeout`, `Switchboard::try_recv_any_with_deadline` returned
  without removing the wakers it had parked on each inbound link (the success path already cleaned
  them up). The stragglers were harmless — a leftover waker can only cause an extra, ignored
  re-poll of the already-resolved future, never a missed wake — but the timeout path now clears
  them too, so the parked-waker bookkeeping is symmetric with the success path. Internal only; no
  API change.

## [0.9.0] - 2026-07-08

### Added

- **`Network::recv_any_with_timeout`** — completes the timeout primitive started in 0.8.2: waits
  for the next packet from *any* party and returns it together with the sender's ID, or fails with
  `NetworkError::Timeout(None)` once the deadline passes (no single party can be blamed for an
  any-party timeout). On the simulator the deadline is the same virtual-clock timer event on the
  switchboard used by `recv_from_with_timeout` — a packet arriving exactly at the deadline wins
  the tie, and a packet arriving later is *not* returned by the timed call (it stays queued for a
  later receive, as the bytes would on a real TCP stream) — so simulated and real deployments keep
  identical semantics; `TcpNetwork` maps the call to `tokio::time::timeout`. Regression tests
  cover the all-silent, late-packet, and prompt-sender cases (`tests/recv_timeout.rs`).
- Module-level documentation for the simulation internals (`channel`, `event`, `executor`,
  `simulator`, `network`, `switchboard`). The switchboard also became a directory module with the
  receive futures in a `recv` submodule — an internal reorganization only (the futures were
  already crate-private; no public path changed).

### Changed

- **Breaking: `NetworkError::Timeout` now carries an `Option<PartyId>`.** `recv_from_with_timeout`
  reports `Some(id)`, identifying the silent party as before; `recv_any_with_timeout` reports
  `None`. Code matching `Timeout(party)` must now match `Timeout(Some(party))`; the error message
  renders both forms ("… from party PartyId(1)" / "… from any party").
- **Breaking: `Network::recv_any` now returns `(PartyId, Packet)`** instead of `(Packet, PartyId)`,
  so the sender-and-packet pair has the same shape across `recv_any` and `recv_any_with_timeout`.
- **Breaking: the `Network` trait gained the required `recv_any_with_timeout` method.** External
  implementors of the trait must add it; the built-in backends (`SimNetwork`, `TcpNetwork`)
  already do, so users of the built-ins are unaffected.

## [0.8.2] - 2026-07-07

### Added

- Added a `recv_from_with_timeout` function in which the receiver waits for a message within
  a timeout. If the message does not arrive in time, the protocol returns a `NetworkError::Timeout`
  with the ID of the delayed party.

## [0.8.1] - 2026-07-06

### Fixed

- **Simulator: per-link deliveries are now FIFO.** The switchboard scheduled every delivery at
  `send time + delay(packet size)` and delivered from a global time-ordered queue, so under a
  size-dependent delay model a _later but smaller_ message could overtake an _earlier but larger_
  one **on the same link** — something a real TCP stream (one connection per peer, as in
  `TcpNetwork`) can never do, so the two backends disagreed. This surfaced as a flaky
  `tests/protocol_share.rs` failure: `postcard`'s varint encoding makes a share's packet size
  value-dependent (a uniform Mersenne-61 element encodes shorter with probability ≈ 1/32), and
  when the dealer's open-share overtook its deal-share, the receiver paired shares with the wrong
  parties and Shamir reconstruction returned a wrong value (additive sharing, being a sum, was
  immune). `Switchboard::send` now clamps each arrival to be no earlier than the previously
  scheduled arrival on the same link (ties keep send order via the sequence number), and a
  regression test pins that a smaller later message does not overtake a larger earlier one.

## [0.8.0] - 2026-07-06

### Added

- **`LinearShare` trait (`ss::LinearShare`)** — a common abstraction over _linear_ secret sharing
  schemes, so a protocol can be written once and run over any of them. It requires the local,
  communication-free operations as operator bounds — `[x] ± [y]` and `-[x]` (share-wise), and
  `[x] ± c` / `c · [x]` (public constant/scalar) — plus `encode_party` (the canonical, injective
  party → field-point map; Shamir places party `i` at the field point `i + 1`, so the usual
  `0`-based network ids never touch the secret's point `0`), `shares_from_secret`, and
  `secret_from_shares` (positional in `parties`, returning a `Result`). Shares are bound
  `Send + Sync + Serialize + DeserializeOwned` so they can travel in `Packet`s inside async
  protocols. Implemented for `ShamirSS` and `AdditiveSS`. Multiplying two shares is deliberately
  excluded: it is non-linear and needs an interactive protocol (e.g. Beaver multiplication).
- **Local linear operators on the share types.** `ShamirSS` and `AdditiveSS` now implement `Add` /
  `Sub` / `Neg` (share-wise) and `Add` / `Sub` / `Mul` by a public constant, matching the
  `LinearShare` contract; share-wise operations debug-assert compatible metadata.
- **Generic deal and open protocols (`protocol::share`)** — the first interactive protocols built
  on `LinearShare`, working over any linear scheme. All assume a **passive (semi-honest)
  adversary** — every party follows the protocol and always sends — reflected in their `Passive*`
  names (receive timeouts and malicious-model variants are roadmap follow-ons): `PassiveDealShr`
  (a designated dealer splits a secret and distributes one share per receiver; receivers are
  constructed without a secret), `PassiveOpenShr` (every party reveals its share to everyone,
  collects one share from each peer, and reconstructs), and `PassiveOpenToParty` (the parties
  reveal their shares towards a single designated party — the common MPC output pattern; only the
  receiver's output is `Some(secret)`). The `protocol` module became a directory (`protocol/`) to
  host them; existing paths are unchanged.
- **New `protocol::Error` variants**: `Share` (a type-erased, boxed `ShareError<T>`, so the
  `Protocol` trait stays independent of any particular ring; downcast to recover the structured
  error) and `Input` (the protocol was constructed with input that does not match its role).
- End-to-end integration tests for the deal/open protocols (`tests/protocol_share.rs`): a
  deal→open roundtrip, an affine `a·[x] + b` computed locally on the shares and then opened, and
  an open-towards-one-party check (`Some(secret)` only at the receiver) — each run on the
  deterministic simulator over both `AdditiveSS` and `ShamirSS`.

### Changed

- **Breaking: `AdditiveSS<T>` now stores its holding party and a leader flag** (it was a bare newtype
  over the value). A public constant is absorbed by a single designated party — the one with the
  smallest id, chosen at dealing time and stamped on every share — so public-constant add/subtract is
  correct for any party numbering. Its inherent `shares_from_secret` consequently takes
  `parties: &[PartyId]` instead of a party count, and `new` takes the party and leader flag.
- **Breaking: `PartyId` now derives `Serialize`, `Deserialize`, and `Ord`** (needed to carry it
  inside an additive share and to select the leader).
- `ShareError::InvalidShare` now formats the offending party index with `Debug` instead of
  `Display` (rings are not required to implement `Display`), so the `protocol::Error` share-error
  conversion — and therefore the open protocols — work with every field type in the crate.

## [0.7.1] - 2026-06-30

### Added

- Tier 4 protocol-layer tests: a reproducibility test asserting that two runs of the same
  simulation produce byte-equal outputs and identical event traces (pinning the deterministic
  executor's run-to-run stability), and a capability-carrying environment test that defines a
  test-only supertrait of `Environment` and a protocol bounded on it, exercising the
  composition capability path.
- Tier 5 real-network tests (inline in `src/net/tcp.rs`): a multi-party (`n > 2`) `recv_any`
  test over real mTLS sockets, plus failure-path coverage for the error variants added in
  0.4.0 — connection closed mid-receive (`ConnectionClosed`), malformed configuration JSON
  (`ConfigParse`), and unloadable PEM material (`InvalidPemFile`).
- Per-constructor documentation examples (compiled doctests) on `Packet::empty`,
  `ShamirSS::{new, shares_from_secret}`, and `FeldmanSS::{new, shares_from_secret}`; the
  share/reconstruct examples double as executable round-trip smoke tests.

### Changed

- Migrated the `tests/simulator.rs` protocols from the concrete `Protocol<GeneralEnv<SimNetwork>>`
  to the generic `impl<E: Environment> Protocol<E>`, using `env.network()`/`env.network_mut()`.
  This compiles and exercises the generic environment path the refactor was built around.

## [0.7.0] - 2026-06-30

### Added

- **`EllipticCurve::is_on_curve`** — a new required trait method that reports whether a point
  satisfies the curve equation. Implemented for `Secp256k1` (via `to_affine().is_valid()`).
- **Property-based test suite (`proptest`).** Added `proptest` as a dev-dependency and a set of
  property tests: ring/field laws (associativity, commutativity, distributivity, identities,
  inverses, subtraction) for Mersenne61 and the secp256k1 prime/scalar fields; Shamir reconstruction
  subset-invariance across random `(secret, t, n)`; polynomial interpolation recovery
  (`interpolate_polynomial_at` agrees with Horner `evaluate` at random points, and recovers the
  constant coefficient at `x = 0`); and `postcard` serialization round-trips for field elements,
  curve points, `ShamirSS`, `FeldmanSS`, and `Packet`. Reusable strategies live in a shared
  `tests/common/mod.rs`. Test-only — no library API change.
- **Negative / adversarial tests for the error paths (testing-plan Tier 3).** `Packet` read/`pop`
  rejections (`EmptyPacket`, `WrongPacketIdx`, and a `postcard` deserialize failure on the wrong
  type — the 0.4.0 `Result` API had no coverage), and the new `interpolate_polynomial_at`
  empty-input and length-mismatch error paths.

### Changed

- **Breaking: the `EllipticCurve<LIMBS>` trait gained a required `is_on_curve` method.** External
  implementors of the trait must add it; the built-in `Secp256k1` already does, so users of the
  built-in curve are unaffected.
- **`interpolate_polynomial_at` now returns errors instead of panicking on malformed input.** Empty
  input and a node/evaluation length mismatch previously tripped `assert!`/`assert_eq!`; they now
  return the new `poly::Error::EmptyInterpolation` and `poly::Error::LengthMismatch` variants, so all
  three of the function's preconditions surface as recoverable errors (the distinct-nodes check
  already did). `poly::Error` is `#[non_exhaustive]`, so the added variants are not breaking.

### Security

- **Feldman VSS now rejects off-curve commitments.** `FeldmanSS::is_valid` validates that every
  dealer-supplied commitment is on the curve (via the new `EllipticCurve::is_on_curve`) before it is
  used in `scalar_mul`, so an adversarial dealer can no longer feed an off-curve point into the
  verification equation; `secret_from_shares` surfaces this as `ShareError::InvalidShare`. Guarded by
  new adversarial tests (off-curve commitment, tampered share, wrong commitment-vector length,
  length mismatch) and point-level on-curve regression tests.

## [0.6.0] - 2026-06-25

### Added

- **Element-type labels in event traces.** A simulation trace's `SEND` and `RECV` lines now report
  a per-type breakdown of the packet's contents in addition to its byte size, e.g.
  `SEND  2 -> 0 (1024 bytes: 1 EC elem., 2 Shamir shr., 4 field elem.)`. On a `RECV` line the labels
  are the sender's, carried in-process by the simulator (which never serializes packets). The labels
  are supplied by
  a new `Abbreviate` trait (`scl_rs::abbreviate`, re-exported from the prelude) whose
  `const ABBREVIATION: &'static str` gives a type its short display label. Built-in types
  (the finite fields, polynomials, vectors, and additive/Shamir/Feldman shares) already implement
  it. Protocols opt in per element via the new `Packet::write_labeled` (vs. `Packet::write`, which
  records the element as `unknown elem.`); `Packet::composition` exposes the `(label, count)`
  breakdown. Labels are display-only metadata: they are `#[serde(skip)]`, so they never cross the
  wire (no bandwidth cost, no effect on packet equality) and the breakdown is available on the
  simulator, which passes packets in-process.
- **`Packet::write_many` and `Packet::write_many_labeled`** — write a slice of values as separate
  packet entries in one call (the bulk counterparts of `write` / `write_labeled`).
- An `examples/send_different_types.rs` example: a two-party protocol that builds a heterogeneous
  packet (a scalar field element, a curve point, and a vector of additive shares), sends it, reads
  the elements back, and prints the per-type-annotated trace. Run with
  `cargo run --example send_different_types`.

### Changed

- **Breaking (module paths): the simulator module `net::simulation::runtime` was renamed to
  `net::simulation::simulator`.** Code that deep-paths to `simulate`/`SimulationOutcome` (e.g.
  `use scl_rs::net::simulation::runtime::simulate;`) must update the path to
  `net::simulation::simulator`. Users importing through `scl_rs::prelude::*` are unaffected.
- The real-TLS backend (`TcpNetwork` and its private `PeerWriter`/`PacketStream` helpers) moved out
  of `net` into a dedicated `net::tcp` module, mirroring the existing `net::simulation` backend.
  `TcpNetwork` is re-exported from `net`, so the `net::TcpNetwork` path (and the prelude) is
  unchanged; this is an internal reorganization with no public API change.
- **Breaking: `Packet`'s internal representation changed** from `Vec<Vec<u8>>` to a private
  `Vec<Element>`, where each element pairs its payload bytes with an optional display-only label
  (see "Added" above). The wire format is unchanged (the label is `#[serde(skip)]`). The previously
  public `Packet::new` is now crate-internal, since constructing a packet goes through
  `Packet::empty` + `write`/`write_labeled`; building from raw bytes is no longer part of the public
  API.
- **Breaking: the simulator's `Event::SendData` and `Event::ReceiveData` each gained a
  `content_count` field** carrying the `(label, count)` breakdown rendered in the `SEND` / `RECV`
  trace line. Code that constructs or exhaustively matches these variants must account for it.

### Removed

- The legacy `Event::HasData` variant (and the matching `EventType::HasData`) were deleted; they
  were not produced by the current event-loop simulator (which has no `has_data` polling).

## [0.5.2] - 2026-06-23

### Added

- `FeldmanSS` now derives `Clone`, so a Feldman share can be duplicated by value — useful for
  rebuilding a share set (e.g. constructing test fixtures or fanning the same share to multiple
  consumers).
- An `examples/real_tls_send_recv.rs` example: the same `SendRecvProtocol` from
  `simple_send_recv.rs` run over a **real two-party mutually-authenticated TLS (mTLS) deployment**
  instead of the simulator, demonstrating that a protocol generic over `E: Environment` runs
  unchanged on either backend. Committed `examples/config_p0.json` and `examples/config_p1.json`
  configuration files, plus module-doc run instructions, make it runnable end to end. Launch with
  `cargo run --example real_tls_send_recv -- <my_id> <config_path>`.

### Fixed

- **`Matrix` indexing was wrong for non-square matrices.** `get`, `get_mut`, matrix×matrix
  multiplication, and matrix×vector multiplication used a row count (`rows`) as the row stride
  instead of the column count (`columns`), so for any matrix where `rows != columns` they read the
  wrong element. The bug was masked by the existing square-only tests. `get`/`get_mut` now also
  bounds-check the row and column indices independently and return `None` when either is out of
  range (previously an out-of-range row could silently return a valid-looking element from another
  row).

## [0.5.1] - 2026-06-22

### Fixed

- Corrected code snippets in the documentation (no API or behavior change).

## [0.5.0] - 2026-06-22

### Changed

- **`Environment<N>` is now a trait, `Environment`, rather than a concrete struct.** The network is reached
  through `fn network(&self) -> &Self::Net` and `fn network_mut(&mut self) -> &mut Self::Net` over an associated `type Net: Network` — the
  one capability every protocol shares, since the same network threads through every layer of a
  composed protocol. Families that need ambient, computation-wide state (e.g. a batched MAC-check
  accumulator) define their own capability traits as **supertraits of `Env`** and bound on them; the
  core crate ships no such capability, keeping `Env` protocol-agnostic.
- **`Protocol` is parameterized by the environment, not the network.** `Protocol<N: Network>` with
  `run(self, &mut Environment<N>)` becomes `Protocol<E: Environment>` with `run(self, &mut E)`. Fully general
  protocols are written `impl<E: Environment> Protocol<E>` and run under any environment; a protocol that
  requires extra capabilities bounds on the corresponding `Environment` supertrait, and those bounds
  accumulate up a composition so the outermost protocol must supply the union of its subtree's
  capabilities — enforced at compile time. **Migration:** replace the `N: Network` parameter with
  `E: Environment`, take `&mut E`, and access the network via `env.network_mut()` and `env.network()` instead of `env.network`.
- **`simulate` is now generic over the environment and takes an environment factory.**
  `simulate<P, E>(config, parties, make_protocol, make_env, hooks)`, where
  `make_env: impl Fn(PartyId, SimNetwork) -> E` constructs each party's environment — the harness can
  no longer build it, because the environment type is open. The bounds are `E: Environment<Net = SimNetwork>`
  (the simulated environment must wrap the simulator's network) and `P: Protocol<E>`, the latter
  propagating each protocol's capability requirements to the factory: a protocol that needs a
  capability will not compile against an environment that does not provide it.
- **`SimulationTrace`'s `Display` now renders as an indented tree** that shows protocol composition,
  rather than a flat one-event-per-line list. Each protocol scope is rendered as a brace block: it
  opens with `<name> {`, indents the events that occur within it, and closes with `}` at the same
  level as the opening line; nested sub-protocol calls indent further, so the tree mirrors the call
  structure.
- **Secret-generation APIs now require a CSPRNG.** `AdditiveSS::shares_from_secret`,
  `ShamirSS::shares_from_secret`, and `FeldmanSS::shares_from_secret` bound their generator on
  `rand::CryptoRng` instead of `rand::Rng`, so secret material can no longer be seeded from a
  predictable (non-cryptographic) generator. **Migration:** pass a cryptographically secure RNG such
  as `rand::rng()` or a `ChaCha20Rng` seeded from OS entropy; a non-CSPRNG generator no longer
  compiles. Lower-level sampling that is not inherently secret (`Ring::random`, `Polynomial::random`,
  `Matrix::random`, `Vector::random`) is unchanged and still accepts any `Rng`.
- **`ChannelId` is replaced by a single directed `Link` type.** The simulator previously had two
  party-pair types — `ChannelId { local, remote }` and an internal routing `Link` — which are now
  collapsed into one directed `Link { sender, recipient }` in
  `net::simulation::channel`. `NetworkConfig::channel_config` now takes a `Link` (`sender` →
  `recipient`) instead of a `ChannelId`, so a configuration can give the two directions of a party
  pair different characteristics (asymmetric up/down links); the `SendData`/`ReceiveData`/etc.
  `Event` variants carry a `link: Link` instead of `channel_id: ChannelId`. **Migration:** in a
  `channel_config` implementation, replace `channel_id.local()`/`.remote()` with
  `link.sender()`/`.recipient()` (loopback is `link.sender() == link.recipient()`); the unused
  `ChannelId::flip_end_points` is removed. Trace rendering is unchanged — links still print from each
  party's own perspective (`sender -> recipient` outgoing, `recipient <- sender` incoming).

### Added

- The `Environment` trait (associated `type Net: Network`, `network_mut`) and `GeneralEnv<N>`, the
  general-purpose environment carrying only the network — the default for protocols that need no
  ambient capability beyond the wire.
- **`Protocol::execute`** — a provided method that invokes a protocol (including a sub-protocol from
  within another protocol's `run`), bracketing it with protocol-scope trace markers. `run` defines a
  protocol's behavior; `execute` invokes it with tracing, so the trace reflects how protocols nest.
  Invoke protocols through `execute` rather than `run` (e.g. `SubProtocol { .. }.execute(env).await?`).
- **`Network::record_protocol_begin` / `record_protocol_end`** — trace hooks called by
  `Protocol::execute`. They default to no-ops; the deterministic simulator overrides them to record
  `ProtocolBegin` / `ProtocolEnd` events, while real-network backends keep no trace and stay no-ops,
  so behavior there is unchanged.
- **`Network::send_many`** — a _scatter_ primitive that sends a batch of `(PartyId, Packet)` messages
  in one round. It is a provided method that defaults to a sequential loop over `send_to`;
  `TcpNetwork` overrides it to drive the per-peer socket writes concurrently (within the task, via
  `try_join_all` — no spawning, so the deterministic simulator still drives it). On the simulator a
  sequential and a concurrent scatter are equivalent (every send is stamped at the sender's current
  virtual instant), so this only speeds up a real deployment while keeping one code path for both. The
  `examples/additive_shr_secure_sum.rs` distribution and reconstruction rounds now use it. (Adds a
  `futures-util` dependency.)
- An `examples/additive_shr_secure_sum.rs` example: an `n`-party secure summation ("hello world" of
  MPC) built on additive secret sharing, composing a sharing-distribution sub-protocol and a
  reconstruction sub-protocol, generic over the environment. Runnable with
  `cargo run --example additive_shr_secure_sum`.

### Removed

- The concrete `Environment<N>` struct and `Environment::new`, superseded by the `Environment` trait and
  `GeneralEnv<N>`. The prelude re-exports `Environment` and `GeneralEnv` in place of the old struct `Environment`.

## [0.4.1] - 2026-06-19

### Added

- An `examples/simple_send_recv.rs` example: a minimal two-party send/receive protocol, written
  generic over `N: Network` and run on the deterministic simulator. Runnable with
  `cargo run --example simple_send_recv`.

## [0.4.0] - 2026-06-19

### Changed

- Added `#[non_exhaustive]` to error enums.
- `Packet::pop` and `Packet::read` now return a `NetworkError` instead of a silent `Option`.
- `Protocol` now consumes `self`. This allows protocol to use non-`Clone` elements inside its
  execution without using `Option` or `Mutex` tricks with interior mutability.
- Tightened the public API surface. Several simulator internals are now `pub(crate)`:
  `Switchboard::send`/`try_recv_any`/`park_any`/`new` and the `Recv`/`RecvAny` receive futures. The
  `Switchboard` type itself, the `TriggeredHook` and `Delay` extension traits, and `Link` remain
  public.
- Updated package version in `Cargo.toml`.
- Added `set_nodelay(true)` to the streams so that it turns off the Nagle's algorithm.
- `TcpNetwork::recv_any` is now implemented; it previously returned `NetworkError::Unsupported`. Every
  peer connection is multiplexed through a cancel-safe length-delimited frame reader (`FramedRead` +
  `StreamMap`), so dropping a `recv_any` future no longer discards a partially-read frame. Internally
  `TcpNetwork` now keeps the per-peer write and read halves keyed by `PartyId` (split out of each TLS
  stream) instead of boxed `Channel`s, and the loop-back path uses an in-process `mpsc` channel.
- `NetworkConfig::new` now returns `Result<Self, NetworkError>` instead of `std::io::Result<Self>`,
  so configuration loading reports errors through the crate error type like the rest of the network
  API. Malformed JSON and unloadable PEM files surface as distinct variants rather than being
  collapsed into an opaque `io::ErrorKind::InvalidInput`.

### Added

- Added a prelude module re-exporting the common types and traits.
- `NetworkError::EmptyPacket` and `NetworkError::WrongPacketIdx`, returned by `Packet::pop` and
  `Packet::read` to distinguish an absent element from a malformed one.
- `NetworkError::ConnectionClosed` and `NetworkError::SendError`, returned by `TcpNetwork` when a peer
  connection is closed during a receive or a loop-back send fails.
- `NetworkError::ConfigParse` and `NetworkError::InvalidPemFile`, returned by `NetworkConfig::new` for
  malformed configuration JSON and unloadable certificate/private-key PEM files, respectively.
- Added small information about benchmarking.
- A `publish-dry-run` CI workflow that runs `cargo publish --dry-run` on version tags (and on manual
  dispatch), guarding releases against packaging regressions. It also fails if any private-key or
  certificate material (`.pem`/`.key`/`.crt`/`.csr`/`.srl`) would be included in the published
  tarball.
- A real-TLS integration test (`tls_public_api_correctness`) that stands up two `TcpNetwork` instances
  over loopback sockets and exercises the public API end to end: handshake, `send_to`/`recv_from`,
  `recv_any` (asserting the sender's `PartyId`), a multi-record frame (a 64 KiB payload spanning
  several TLS records, to cover the length-delimited reader's cross-read reassembly), and `close`.

### Removed

- Removed the vestigial `Channel` trait (and its blanket implementation), `LoopBackChannel`, and the
  `ChannelError::EmptyBuffer` variant. They are superseded by the framed `TcpNetwork` transport
  (`FramedRead` + `StreamMap` for sockets, an in-process `mpsc` channel for loop-back).

### Fixed

- Corrected `gen_self_signed_certs.sh`: leaf certificates are now signed only by the root CA (the
  redundant self-signed step that was immediately overwritten is gone) and carry both `serverAuth` and
  `clientAuth` extended-key-usages so the same certificate works in both mTLS roles. The script now
  validates its `<n_parties>` argument, fails fast on errors, drops the unused `DNS:server` subject
  alternative name (only `IP:127.0.0.1` is used), and cleans up the intermediate CSR/serial files.

## [0.3.1] - 2026-06-17

Documentation-only release; no functional or API changes.

### Documentation

- Documented that networking uses **mutual TLS (mTLS)** — in the README, crate-level docs, and the
  network-configuration field descriptions. The `0.3.0` code already required mTLS, but the prose
  still described one-way TLS.
- Documented `Network::recv_any` in the crate/README introduction and the `SimNetwork` docs.
- Recorded the project's versioning stance: scl-rs stays on `0.x` indefinitely with no planned `1.0`
  release (README, crate docs, `SECURITY.md`), and reframed `docs/roadmap.md` accordingly.

## [0.3.0] - 2026-06-17

### Changed

- **Networking now requires mutual TLS (mTLS).** Each peer presents its own
  certificate as a client identity (`with_client_auth_cert`) and verifies the
  remote peer's certificate against the trusted root store
  (`WebPkiClientVerifier`). Previously the client side performed one-way
  authentication only. **This changes the wire protocol: nodes running this
  version cannot complete a TLS handshake with `0.2.0` nodes.**
- Updated dependencies following a `cargo audit` review.
- Network configuration files are now parsed with a typed `serde` deserializer
  instead of manual JSON walking. Unknown or misspelled keys are now rejected
  (`deny_unknown_fields`) rather than silently ignored.

### Added

- Declared a Minimum Supported Rust Version (MSRV) of 1.85.1, enforced by a
  dedicated CI job.
- `NetworkError::VerifierBuilderError` variant, returned when the client
  certificate verifier cannot be constructed.
- `Network::recv_any`, which receives the next packet from whichever peer
  delivers first, returning the sender's `PartyId` alongside it. This is the
  building block for quorum-based protocols such as reliable broadcast, which
  wait for the first `k`-of-`n` messages and must not block on the parties that
  stay silent. It is currently implemented for the simulator; on `TcpNetwork` it
  returns an error pending a cancellation-safe multiplexed receive path.
- `NetworkError::Unsupported` variant, returned by a network operation that a
  backend does not yet implement.
- Added test for correctness of the TLS handshake.

### Fixed

- Corrected the installation instructions in the documentation.
- A `base_port` outside the `u16` range in a configuration file is now rejected
  with an error instead of being silently truncated.

## [0.2.0] - 2026-06-16

### Added

- Continuous integration with separate `fmt`, `clippy`, `test`, and `docs` jobs.
- `SECURITY.md` with the project's security posture and threat model, plus a
  pre-1.0 / unaudited security disclaimer.
- `Network: Send` as a supertrait of `Network`, so protocols written generic
  over `N: Network` compile without an explicit `+ Send` bound.

### Changed

- `simulate` now takes the parties and a protocol factory as separate arguments
  (`parties: Vec<PartyId>` plus `make_protocol: impl Fn(PartyId) -> P`) instead
  of a `Vec<(PartyId, P)>`.
- Crate-level documentation examples are now compiled doctests.
- Corrected and expanded the real-network documentation (two-party JSON
  configuration, two-process run instructions, certificate generation).

### Removed

- `Environment::clock` and the vestigial wall-clock `Clock` type. `Environment`
  is now solely the network seam (`{ network: N }`).

### Fixed

- Isolated the simulator integration tests so the suite no longer compiles and
  runs twice.

## [0.1.0] - 2026-06-12

Initial release, published to [crates.io](https://crates.io/crates/scl-rs).

### Added

- **Finite field arithmetic** — a `FiniteField` trait, the Mersenne-61 field
  (`Z_p` with `p = 2^61 - 1`), and the secp256k1 base and scalar fields.
- **Elliptic curves** — secp256k1 in affine coordinates.
- **Polynomials** over arbitrary rings, with Lagrange interpolation over finite
  fields.
- **Linear algebra** — matrices and vectors over arbitrary rings.
- **Secret sharing** — additive, Shamir, and Feldman verifiable secret sharing.
- **Networking** — point-to-point channels over TCP secured with TLS
  (`tokio-rustls`), using length-prefixed framing.
- **Typed protocol framework** — the `Protocol` trait with an associated
  `Output` type and an `async fn run`; protocols compose by calling one another
  and using each other's typed return values.
- **Deterministic discrete-event simulator** — a single-threaded executor that
  drives protocols on a virtual clock with configurable latency and bandwidth,
  producing reproducible results and per-party event traces. The simulator and a
  real deployment share one `Network` trait, so a protocol runs on either
  unchanged.

[Unreleased]: https://github.com/hdvanegasm/scl-rs/compare/v0.15.0...HEAD
[0.15.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.14.2...v0.15.0
[0.14.2]: https://github.com/hdvanegasm/scl-rs/compare/v0.14.1...v0.14.2
[0.14.1]: https://github.com/hdvanegasm/scl-rs/compare/v0.14.0...v0.14.1
[0.14.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.13.0...v0.14.0
[0.13.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.12.1...v0.13.0
[0.12.1]: https://github.com/hdvanegasm/scl-rs/compare/v0.12.0...v0.12.1
[0.12.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.11.2...v0.12.0
[0.11.2]: https://github.com/hdvanegasm/scl-rs/compare/v0.11.1...v0.11.2
[0.11.1]: https://github.com/hdvanegasm/scl-rs/compare/v0.11.0...v0.11.1
[0.11.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.10.0...v0.11.0
[0.10.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/hdvanegasm/scl-rs/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.8.2...v0.9.0
[0.8.2]: https://github.com/hdvanegasm/scl-rs/compare/v0.8.1...v0.8.2
[0.8.1]: https://github.com/hdvanegasm/scl-rs/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.7.1...v0.8.0
[0.7.1]: https://github.com/hdvanegasm/scl-rs/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.5.2...v0.6.0
[0.5.2]: https://github.com/hdvanegasm/scl-rs/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/hdvanegasm/scl-rs/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/hdvanegasm/scl-rs/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/hdvanegasm/scl-rs/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/hdvanegasm/scl-rs/releases/tag/v0.1.0
