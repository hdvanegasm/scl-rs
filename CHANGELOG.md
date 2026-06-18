# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
scl-rs stays on `0.x` indefinitely (there is no planned `1.0`); breaking changes may occur in any
`0.x` release and are bumped in the minor position (`0.y`).

## [Unreleased]

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

### Added

- Added a prelude module re-exporting the common types and traits.
- `NetworkError::EmptyPacket` and `NetworkError::WrongPacketIdx`, returned by `Packet::pop` and
  `Packet::read` to distinguish an absent element from a malformed one.
- Added small information about benchmarking.

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

[Unreleased]: https://github.com/hdvanegasm/scl-rs/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/hdvanegasm/scl-rs/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/hdvanegasm/scl-rs/releases/tag/v0.1.0
