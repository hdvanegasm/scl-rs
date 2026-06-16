# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While the project is pre-1.0, breaking changes may occur in any `0.x` release and
are bumped in the minor position (`0.y`).

## [0.3.0] - 2026-06-16

### Changed

- **Networking now requires mutual TLS (mTLS).** Each peer presents its own
  certificate as a client identity (`with_client_auth_cert`) and verifies the
  remote peer's certificate against the trusted root store
  (`WebPkiClientVerifier`). Previously the client side performed one-way
  authentication only. **This changes the wire protocol: nodes running this
  version cannot complete a TLS handshake with `0.2.0` nodes.**
- Updated dependencies following a `cargo audit` review.

### Added

- Declared a Minimum Supported Rust Version (MSRV) of 1.85.1, enforced by a
  dedicated CI job.
- `NetworkError::VerifierBuilderError` variant, returned when the client
  certificate verifier cannot be constructed.

### Fixed

- Corrected the installation instructions in the documentation.

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

[Unreleased]: https://github.com/hdvanegasm/scl-rs/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/hdvanegasm/scl-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/hdvanegasm/scl-rs/releases/tag/v0.1.0
