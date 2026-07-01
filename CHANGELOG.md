# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
scl-rs stays on `0.x` indefinitely (there is no planned `1.0`); breaking changes may occur in any
`0.x` release and are bumped in the minor position (`0.y`).

## [Unreleased]

### Added

- **`LinearShare` trait (`ss::LinearShare`)** — a common abstraction over *linear* secret sharing
  schemes, so a protocol can be written once and run over any of them. It requires the local,
  communication-free operations as operator bounds — `[x] ± [y]` and `-[x]` (share-wise), and
  `[x] ± c` / `c · [x]` (public constant/scalar) — plus `encode_party` (the canonical, injective
  party → field-point map), `shares_from_secret`, and `secret_from_shares` (positional in `parties`,
  returning a `Result`). Implemented for `ShamirSS` and `AdditiveSS`. Multiplying two shares is
  deliberately excluded: it is non-linear and needs an interactive protocol (e.g. Beaver
  multiplication).
- **Local linear operators on the share types.** `ShamirSS` and `AdditiveSS` now implement `Add` /
  `Sub` / `Neg` (share-wise) and `Add` / `Sub` / `Mul` by a public constant, matching the
  `LinearShare` contract; share-wise operations debug-assert compatible metadata.

### Changed

- **Breaking: `AdditiveSS<T>` now stores its holding party and a leader flag** (it was a bare newtype
  over the value). A public constant is absorbed by a single designated party — the one with the
  smallest id, chosen at dealing time and stamped on every share — so public-constant add/subtract is
  correct for any party numbering. Its inherent `shares_from_secret` consequently takes
  `parties: &[PartyId]` instead of a party count, and `new` takes the party and leader flag.
- **Breaking: `PartyId` now derives `Serialize`, `Deserialize`, and `Ord`** (needed to carry it
  inside an additive share and to select the leader).

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

[Unreleased]: https://github.com/hdvanegasm/scl-rs/compare/v0.7.1...HEAD
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
