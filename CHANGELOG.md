# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
scl-rs stays on `0.x` indefinitely (there is no planned `1.0`); breaking changes may occur in any
`0.x` release and are bumped in the minor position (`0.y`).

## [Unreleased]

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

[Unreleased]: https://github.com/hdvanegasm/scl-rs/compare/v0.12.1...HEAD
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
