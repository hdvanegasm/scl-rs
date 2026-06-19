# scl-rs Development Roadmap

**Date:** 2026-06-17

**Current version:** 0.4.0 (**published to crates.io on 2026-06-19**; tagged `v0.4.0`).

**Versioning stance:** scl-rs stays on **`0.x` indefinitely**. `1.0` is **not a planned milestone** —
the "unaudited / not for production" posture is carried by the security disclaimer (not the version
number), and the API has no downstream usage yet to justify freezing it. See §2.

**Goal:** evolve toward a **stable, well-baked `0.x` API** — one that settles across releases and
breaks only rarely — while keeping the library useful for prototyping MPC protocols.

This document is a living plan. It captures where the library is today, the work grouped into themed
workstreams, a suggested version sequence, and a **Definition of a stable `0.x`** (§12).

---

## 1. Current state (honest snapshot)

**What exists and works** (`cargo build`, `cargo test`, `cargo doc -D warnings` all clean; ~4 kLOC):

- **`math`** — `Ring` and `FiniteField<const LIMBS>` traits; Mersenne‑61 field; secp256k1 base
  field, scalar field, and curve; rings, polynomials (Lagrange interpolation), matrices, vectors,
  NAF. Reasonable unit-test coverage.
- **`ss`** — additive sharing, Shamir, Feldman VSS, with a generic `ShareError<T: Ring>`.
- **`net`** — real TLS point-to-point networking (`TcpNetwork` over `tokio-rustls`) **and** a
  single-threaded deterministic discrete-event simulator (`SimNetwork` + `Switchboard` + virtual
  clock), both behind one `Network` trait.
- **`protocol`** — one `Protocol<N>` trait with a typed `Output`; protocols compose by direct
  `.await`; `simulate<P>` runs them deterministically and returns typed outputs + event traces.

**Publishability is now cleared** (see §4): `Cargo.toml` has `license`, `description`, `keywords`,
`categories`, `repository`, `readme`; tokio features are narrowed; `certs/` and the generator script
are excluded; `cargo publish --dry-run` passes; and the TLS `send` flush (§7) is fixed. A first `0.x`
release can go out now.

**What remains** is mostly _productization_, not core features: API stabilization, hardening, and
docs/examples. (MSRV, the security disclaimer, and `SECURITY.md` are now in place.) Those are the body
of this roadmap — work that improves the `0.x` line, not a checklist gating a `1.0`.

---

## 2. Versioning stance — perpetual `0.x` (decided)

**Decision: scl-rs stays on `0.x` indefinitely; `1.0` is explicitly optional and not planned.**

SemVer still applies — `0.x` _is_ SemVer. Within a `0.y` line Cargo guarantees patch/minor
compatibility, and a breaking change bumps the minor (`0.y → 0.(y+1)`). We follow that discipline
because the registry's resolver relies on it; the only choice is whether our bumps tell the truth.
What we are _not_ doing is making the `1.0` promise ("no breaks until 2.0").

Why stay on `0.x`:

1. **The version number tracks API compatibility, not security trust.** "Unaudited / not for
   production" is communicated by the README banner, crate-doc disclaimer, and `SECURITY.md` — the
   right channel for it. Refusing to pass `1.0` to signal "not audited" overloads a number that is bad
   at carrying that meaning (plenty of `0.x` crates ship in production; plenty of `1.0` crates are
   toys). These are orthogonal axes; let each be carried by the right mechanism.
2. **The honest precondition for `1.0` is "the API has been used by someone and survived."** This
   crate has ~zero downstream users; until real usage pressure-tests the API, freezing it is premature
   — independent of auditing. For a niche AGPL research tool this may never change, and that is fine.
3. **The cost of staying on `0.x` is low.** Cargo gives patch/minor compatibility _within_ a `0.y`
   line, and serious crates live on `0.x` for years. The main cost — some consumers read `0.x` as "not
   ready" — barely applies here, since the AGPL + unaudited-research framing already says so out loud.

What "stable `0.x`" means in practice: do the §5 API-stabilization work, let the API **bake** across a
few releases, then break only rarely and deliberately. This is where `#[non_exhaustive]` on public
enums earns its keep — it turns new variants into _patch_ releases instead of forced minor bumps. A
patch-mostly `0.x` line that breaks seldom is the intended terminal state, not a way-station to `1.0`.

**`1.0` remains available, never owed.** If a concrete reason ever appears — a real user asks for the
stability guarantee, or a deliberate decision is made about what `1.0` should claim (pure
API-stability vs an actual audit) — it can be revisited then. Until then it is off the table, and
nothing in this roadmap is gated on it.

---

## 3. Two gating decisions (resolve first — they color everything else)

### D-A. License posture — **DECIDED: AGPL‑3.0-or-later**

**Resolved.** `Cargo.toml` now declares `license = "AGPL-3.0-or-later"` (valid SPDX; the deprecated
plain `AGPL-3.0` form is avoided), matching the `LICENSE` file. The publish blocker is cleared.

The trade-off was made deliberately: AGPL is the most restrictive common OSS license — it obliges
anyone who _uses the library over a network_ to release their source — and companies routinely ban
AGPL dependencies, so this will deter most downstream commercial adoption. That copyleft posture is
intentional. If broad adoption later becomes the goal, the Rust-ecosystem norm is dual
`MIT OR Apache‑2.0`; as sole copyright holder the author can relicense _future_ versions freely,
though any already-published version stays under the license it shipped with.

### D-B. Security posture & audit status

This is cryptography / MPC code. Going public without a clear posture is itself a risk.

- Ship a prominent **security disclaimer** ("research / prototyping; **not audited**; not for
  production use") in `README.md`, crate docs, and a `SECURITY.md`.
- Decide the **threat model** you claim (honest-but-curious? malicious? side-channel resistance?) and
  state it. Today the arithmetic uses **variable-time** sampling (`Uint::random_mod_vartime` in the
  secp256k1 fields) and the secret-sharing APIs accept any `Rng` (not necessarily a CSPRNG) — so the
  honest current claim is "no side-channel guarantees." See §6.

---

## 4. Workstream — Publishability (make `cargo publish` succeed)

Mechanical, do first; unblocks an early `0.x` on crates.io. **Essentially complete** — the dry-run
passes and the package is clean. Only MSRV and the docs.rs verification remain.

- [x] Add `license` to `Cargo.toml` — `license = "AGPL-3.0-or-later"` (see D-A).
- [x] Add `description` — present.
- [x] `repository`, `readme`, `keywords`, `categories` — present. (`documentation`/`homepage` are
      optional niceties; docs.rs is inferred from the crate name.)
- [x] Narrow tokio features: now `tokio = { features = ["net", "io-util", "time", "rt"] }` (down from
      `"full"`); `cargo build`/`cargo test` green. `rt` is needed only by the unused
      `JoinHandleError` variant — drop that variant and `rt` can go too.
- [x] `cargo publish --dry-run` clean; `exclude = ["certs/", "gen_self_signed_certs.sh"]` keeps the
      private keys and generator script out of the tarball (`cargo package --list` confirms no
      `.pem`/`.key`/`.crt` ship).
- [x] Declare an **MSRV**: `rust-version = "1.XX"` and test it in CI.
- [x] Verify the docs.rs build (it builds on a fixed toolchain) after the first publish.

## 5. Workstream — API stabilization (toward a stable `0.x` API)

Each item below is a breaking change. On `0.x` these stay relatively cheap, but the aim is to land
them, let the API **bake**, and then break only rarely — so do them deliberately and batch them per
release rather than dribbling breaks out continuously.

- [x] **`Packet` consumer API is error-swallowing — fixed in 0.4.0.** `read(idx)` and `pop()` now
      return `Result<T, NetworkError>` (`EmptyPacket`/`WrongPacketIdx`), so consumers can distinguish
      "absent" from "malformed." (`src/net/mod.rs`.)
- [x] **Error-type consistency sweep — done in 0.4.0.** `#[non_exhaustive]` added to the public error
      enums (new variants become patch-level changes), and `NetworkConfig::new` now returns
      `Result<Self, NetworkError>` instead of leaking `std::io::Result` — malformed JSON and unloadable
      PEM files surface as `NetworkError::ConfigParse`/`InvalidPemFile` rather than an opaque
      `io::ErrorKind::InvalidInput`.
- [x] **`Protocol` receiver decision — settled in 0.4.0.** `Protocol::run` now consumes `self`, letting
      a protocol move non-`Clone` inputs into `run` without `Option`/`Mutex` interior-mutability tricks.
- [x] **`Network: Send` supertrait added.** `#[async_trait]` makes `Protocol::run`'s future `Send`,
      which needs `Environment<N>: Send` → `N: Send`. Without the bound, generic `impl<N: Network>
  Protocol<N>` did not compile (the crate-doc examples were `ignore`, hiding it). `Network` now
      requires `Send`, so generic protocols are written as `impl<N: Network> Protocol<N>` with no extra
      bound; both `SimNetwork` and `TcpNetwork` already satisfy it. (`src/net/mod.rs`.) The crate-doc
      protocol + simulator examples are now **compiled** doctests (the simulator one runs and asserts),
      so this class of rot is caught going forward; `async-trait` was added to `[dev-dependencies]`.
- [x] **`Environment::clock()` removed.** The vestigial wall-clock `Clock` (it reported real elapsed
      time, meaningless under the deterministic simulator) and its accessor are gone; `Environment<N>`
      is now just `{ pub network: N }` — kept deliberately as the ambient-context seam so future
      execution-wide resources (e.g. a CSPRNG handle, §6) can be added without changing the `Protocol`
      signature. If protocols ever need simulated time, expose it via `Network::now()`
      (`Switchboard::clock_of`) rather than reviving the wall clock. (`src/protocol.rs`.)
- [x] **`simulate<P>` ergonomics — settled.** Signature is now
      `simulate(config, parties: Vec<PartyId>, make_protocol: impl Fn(PartyId) -> P, hooks)`: a
      per-party **factory closure** instead of `Vec<(PartyId, P)>`. All parties still share one
      concrete type `P` (monomorphization), but the factory keeps the per-party construction seam —
      symmetric protocols are `|_| Proto`, and private inputs are `|pid| Proto { input: inputs[&pid] }`
      — without `P: Clone` or per-party boilerplate. (`src/net/simulation/runtime.rs`.)
- [x] **Re-exports / prelude — added in 0.4.0.** A `prelude` module now re-exports the common path so
      users aren't deep-pathing.
- [x] **Naming/visibility audit — done in 0.4.0.** Simulator internals demoted to `pub(crate)`
      (`Switchboard::send`/`try_recv_any`/`park_any`/`new`, the `Recv`/`RecvAny` futures); the vestigial
      `Channel` trait and `LoopBackChannel` removed. (The `ss::ec` doc nit was already moot — `math/ec`
      uses `//!` correctly.)

## 6. Workstream — Crypto & security hardening

- [ ] **CSPRNG bounds for secret material.** Secret-generation APIs (`AdditiveShares::shares_from_secret`,
      Shamir/Feldman) accept any `R: Rng`. Bound them on `R: CryptoRng + RngCore` (or document loudly)
      so callers can't accidentally seed secrets from a predictable PRG.
- [ ] **Constant-time review.** secp256k1 field sampling uses `random_mod_vartime`; audit field/curve
      ops for data-dependent timing on secret inputs. Either provide constant-time paths or document
      the absence of side-channel resistance precisely.
- [ ] **Supply-chain hygiene.** Add `cargo-audit` (RUSTSEC advisories) and/or `cargo-deny` (licenses +
      advisories + bans) to CI.
- [ ] **Threat-model doc** stating what each primitive does and does not guarantee (ties to D-B).

## 7. Workstream

- [x] **`channel.rs::send` flushes after its writes.** Now does `write_all(len)` → `write_all(bytes)`
      → `flush().await?` (channel.rs:77-79), fixing the `tokio-rustls` ciphertext-buffering stall on a
      strict request→response over real TLS. `connect_as_client` also flushes after sending the id.
- [x] **Real-TLS integration test — added in 0.4.0.** `tls_public_api_correctness` stands up two
      `TcpNetwork` instances over loopback sockets (on a dynamically discovered free port) and exercises
      the public API end to end: handshake, `send_to`/`recv_from`, `recv_any` (asserting the sender's
      `PartyId`), a multi-record 64 KiB frame (covering the length-delimited reader's cross-read
      reassembly), and `close`.
- [x] **`recv_any` — receive from any peer (quorum primitive).** `Network::recv_any` returns the next
      packet from whichever peer delivers first (`(Packet, PartyId)`) — the building block for
      quorum-based protocols (reliable broadcast: wait for the first `k`-of-`n`, never block on the
      parties that stay silent). **Implemented for `SimNetwork`** in 0.3.0 (deterministic,
      lowest-sender-id tie-break; `RecvAny` + `try_recv_any`/`park_any` in `switchboard.rs`), guarded
      by regression tests in `tests/simulator.rs`.
  - [x] **`TcpNetwork::recv_any` — implemented in 0.4.0.** It previously returned
        `NetworkError::Unsupported`. The cancel-safety problem (the old `Channel::recv` read a length
        prefix + payload in two `read_exact`s) is solved by wrapping each peer's **split read half** in
        a `FramedRead<_, LengthDelimitedCodec>` and polling all of them through a `StreamMap`: a frame
        stays buffered across polls, so a dropped `recv_any` branch no longer desyncs the stream. The
        loop-back peer is an in-process `mpsc` channel; `recv_from(p)` polls a single entry. (The
        task-per-peer + shared-mpsc design originally sketched here was unnecessary — `StreamMap`
        provides the multiplexing directly.)
  - [ ] **Straggler / virtual-time regression test (sim).** Pin the property that a message from a
        slow party delivered *after* the receiver already passed its quorum does not distort the
        receiver's virtual time (delivery bumps `clock` in `deliver_next`, but it is inert once the
        party is done, and post-quorum synchronous work is stamped before any further delivery).
- [x] **Trace `channel_id` perspective bug.** _Resolved._ `Switchboard::send`/`try_recv` now record
      events with `ChannelId::new(recorder, peer)` (recorder in the `local` slot), so `Event::Display`
      renders arrows from each party's own perspective; the canonical `link.channel_id()` survives only
      in `ConfigDelay::delay`, where the symmetric lookup is intended. Guarded by the
      `trace_arrows_reflect_each_party_perspective` regression test (`tests/simulator.rs`).
- [ ] **Nested protocol calls are invisible in traces** — only the top-level protocol records
      `ProtocolBegin`/`End`. Decide whether nested `.await` calls should appear (needs a recording
      hook reachable from the network-generic `Environment`).
- [ ] **D10 unification.** Collapse the duplicated `Link {recipient,sender}` and
      `ChannelId {local,remote}` + `flip_end_points` into one directed pair type; re-key
      `NetworkConfig::channel_config` to `Link` (also enables asymmetric up/down links).

## 8. Workstream — Quality gates & CI

CI now runs separate fmt / clippy / test / doc jobs (the `module_inception` and `needless_borrow`
lints were cleared; `tests/simulator/` was flattened to a single `tests/simulator.rs`):

- [x] `cargo fmt --all --check`.
- [x] `cargo clippy --all-targets -- -D warnings` (green; pre-existing style lints cleared).
- [x] `cargo doc --no-deps -D warnings` in CI (keep intra-doc links honest).
- [x] `cargo test` on stable. _(MSRV matrix still pending — needs `rust-version` first; see §4/0.3.0.)_
- [x] `cargo publish --dry-run` on tags — added in 0.4.0 (`.github/workflows/publish-dry-run.yml`,
      triggered on `v*` tags and manual dispatch). It also fails the job if any private-key/certificate
      material would be packaged.
- [ ] Optional: coverage reporting; `cargo-audit`/`deny` (see §6).

## 9. Workstream — Docs, examples & ecosystem

- [ ] **`examples/` directory** (none today). At minimum: (a) a simulator run, (b) a real two-party
      TLS deployment (the binary sketched in the crate docs), (c) a secret-sharing round-trip.
      Runnable examples are the fastest on-ramp for new users.
- [x] **`CHANGELOG.md`** (Keep a Changelog format) from 0.1.0 onward.
- [x] **`SECURITY.md`** added (status/posture + threat model & known limitations: variable-time
      sampling, non-CSPRNG `Rng` inputs, unaudited). Reporting channel is public GitHub issues for now
      (acceptable for a research tool); a private channel can be added if the posture changes.
- [ ] **`CONTRIBUTING.md`**.
- [ ] Refresh `README.md`'s "Missing features" into a link to this roadmap; keep the security banner
      at the top.
- [ ] Optional rename `runtime.rs` → `simulator.rs` (cosmetic and breaking — module paths are public,
      so batch it with other §5 breaks if done at all).

## 10. Workstream — Feature completeness (scope to taste)

These are not strictly required, but shape how "complete" the stable `0.x` surface feels.

- [ ] **Arbitrary prime-`p` field** (open README item): a general `F_p` instead of only the
      hand-written Mersenne‑61 / secp256k1 fields.
- [ ] **Test-coverage gap** (open README item): "write missing tests for all functionalities" —
      especially `net` (real path), `matrix`/`poly` edge cases, and serialization round-trips.
- [ ] Any additional MPC facilities you want in the stable surface (e.g. opening/reconstruction
      helpers, a Beaver-triple/multiplication example to showcase typed composition end-to-end).

---

## 11. Suggested release sequence

Ship early and often on `0.x`; let the API bake and then break only rarely. There is no `1.0` row — a
stable, patch-mostly `0.x` is the intended terminal state (§2).

| Version         | Theme                                              | Contents                                                                                                                                                                                                                                                    |
| --------------- | -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **0.2.0**       | _Publishable & honest_ ✅ **PUBLISHED 2026-06-16** | §4 metadata/license/tokio features, the §7 `flush` fix, `SECURITY.md`, compiled doctests, `Network: Send`, factory `simulate`, §8 CI (fmt/clippy/test/doc), and corrected real-network docs. **First crates.io release**, tagged `v0.2.0`, an early `0.x` release. |
| **0.3.0**       | _Correct & clean_ ✅ **PUBLISHED 2026-06-17**      | Mutual TLS (mTLS) — wire-incompatible with 0.2.0; MSRV 1.85.1 + MSRV CI job; typed `serde` config parsing (`deny_unknown_fields`, `base_port` range check); `channel_id` perspective bug resolved + regression test; mTLS handshake tests (positive + negative); `Network::recv_any` **simulator-only** (quorum primitive). Tagged `v0.3.0`.                  |
| **0.4.0**       | _API stabilization_ ✅ **PUBLISHED 2026-06-19**    | §5 in full (Packet `Result` API, error sweep incl. `NetworkConfig::new` → crate error, `Protocol` consumes `self`, `Environment` clock, prelude, naming/visibility audit). Plus the §7 `TcpNetwork::recv_any` implementation (cancel-safe `FramedRead` + `StreamMap` multiplexing) and the `tls_public_api_correctness` real-TLS socket integration test, and the §8 `publish-dry-run` tag workflow. Tagged `v0.4.0`. |
| **0.x**         | _Hardening & completeness_                         | §6 (CSPRNG bounds, constant-time review, cargo-audit), §9 examples/docs, chosen §10 features.                                                                                                                                                               |
| **0.x (stable)** | _API settled — steady state_                      | The §5 work has baked, public enums are `#[non_exhaustive]`, docs/examples are complete, and breaks are rare and deliberate. This is the intended steady state; `1.0` stays optional and unplanned (§2).                                                     |

---

## 12. Definition of a stable `0.x`

The bar for considering the `0.x` API "settled" — the steady state of §11, not a `1.0` gate:

- [ ] License recorded in `Cargo.toml` and consistent with `LICENSE`. _(Done — AGPL-3.0-or-later.)_
- [ ] Security posture + threat model documented; `SECURITY.md` present; disclaimer prominent.
- [x] Public API reviewed and deliberately settled: `Packet` reads return `Result`; public error enums
      `#[non_exhaustive]`; `Protocol` receiver and `Environment::clock` settled; prelude in place. _(Done
      across 0.2.0–0.4.0.)_
- [ ] Secret-generation APIs require a CSPRNG (or the limitation is documented as a conscious choice).
- [ ] All §7 correctness loose ends closed. _(TLS `flush` (0.2.0) and the real-TLS integration test
      (0.4.0) are done; nested-call trace visibility and the D10 link unification remain.)_
- [ ] CI green on fmt, `clippy -D warnings`, `doc -D warnings`, tests across MSRV+stable,
      `publish --dry-run`.
- [ ] `examples/` cover simulator + real deployment + secret sharing; `CHANGELOG.md` current.
- [ ] `docs.rs` renders cleanly; README on-ramp is accurate end to end.

---

## 13. Deferred (later `0.x` or beyond)

- Nested-call trace visibility (§7).
- Adversarial/reordering simulation harness (delay/drop/reorder deliveries) — a payoff of the
  explicit-blocking-state design (`Poll::Pending` = "party blocked on recv").
- Packet loss / retransmission modeling in the event loop.
- Compute-time / sender-side cost modeling in the virtual clock.
- Broader MPC protocol library on top of the typed-composition core.
