# scl-rs Development Roadmap

**Date:** 2026-06-22

**Current version:** 0.4.1 (**published to crates.io on 2026-06-19**; tagged `v0.4.1`) — a patch
release adding the `examples/simple_send_recv.rs` example on top of 0.4.0.

**Unreleased (staged on `main` for the next `0.x`):** an environment-trait redesign (`Protocol<E:
Environment>`, `simulate<P, E>` with an environment factory, `GeneralEnv`), `Protocol::execute` with
a nesting-aware brace-block trace `Display`, CSPRNG bounds on the secret-generation APIs, a
`cargo-audit` CI workflow, a straggler/virtual-time regression test, and the D10 `Link` unification.
These are breaking, so per the §2 policy the next release is a minor bump (`0.5.0`). See
`CHANGELOG.md`.

**Versioning stance:** scl-rs stays on **`0.x` indefinitely**. `1.0` is **not a planned milestone** —
the "unaudited / not for production" posture is carried by the security disclaimer (not the version
number), and the API has no downstream usage yet to justify freezing it. See §2.

**Goal:** evolve toward a **stable, well-baked `0.x` API** — one that settles across releases and
breaks only rarely — while keeping the library useful for prototyping MPC protocols.

This document is a living plan. It captures where the library is today, the work grouped into themed
workstreams, a suggested version sequence, and a **Definition of a stable `0.x`** (§12).

---

## 1. Current state (honest snapshot)

**What exists and works** (`cargo build`, `cargo test`, `cargo doc -D warnings` all clean; ~4.7 kLOC):

- **`math`** — `Ring` and `FiniteField<const LIMBS>` traits; Mersenne‑61 field; secp256k1 base
  field, scalar field, and curve; rings, polynomials (Lagrange interpolation), matrices, vectors,
  NAF. Reasonable unit-test coverage.
- **`ss`** — additive sharing, Shamir, Feldman VSS, with a generic `ShareError<T: Ring>`.
- **`net`** — real TLS point-to-point networking (`TcpNetwork` over `tokio-rustls`) **and** a
  single-threaded deterministic discrete-event simulator (`SimNetwork` + `Switchboard` + virtual
  clock), both behind one `Network` trait.
- **`protocol`** — a `Protocol<E: Environment>` trait with a typed `Output`; protocols compose by
  calling one another through `Protocol::execute` (which brackets each call with trace markers).
  `Environment` is the ambient-context seam (`GeneralEnv` is the default), and `simulate<P, E>` runs
  protocols deterministically and returns typed outputs + nesting-aware event traces.

**Published and iterating** (see §4): releases `0.2.0`–`0.4.1` have shipped to crates.io. `Cargo.toml`
has `license`, `description`, `keywords`, `categories`, `repository`, `readme`; tokio features are
narrowed; `certs/` and the generator script are excluded; `cargo publish --dry-run` passes in CI; MSRV
is pinned at 1.85.1; and the security disclaimer + `SECURITY.md` are in place. A substantial breaking
redesign is now staged unreleased on `main` for the next `0.x` (see the header note).

**What remains** is mostly _productization_, not core features: finishing the §6 hardening
(constant-time review — deferred; threat-model doc), §9 docs (`CONTRIBUTING.md`, a real-TLS example),
and chosen §10 features. Those are the body of this roadmap — work that improves the `0.x` line, not a
checklist gating a `1.0`.

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
  secp256k1 fields), so the honest current claim is "no side-channel guarantees" — the
  secret-generation APIs now require a CSPRNG (§6), but that addresses predictability, not timing.
  This posture is stated in `SECURITY.md`. See §6.

---

## 4. Workstream — Publishability (make `cargo publish` succeed)

Mechanical, do first; unblocked an early `0.x` on crates.io. **Complete** — every item below is done:
the dry-run passes, the package is clean, MSRV is pinned, and docs.rs renders.

- [x] Add `license` to `Cargo.toml` — `license = "AGPL-3.0-or-later"` (see D-A).
- [x] Add `description` — present.
- [x] `repository`, `readme`, `keywords`, `categories` — present. (`documentation`/`homepage` are
      optional niceties; docs.rs is inferred from the crate name.)
- [x] Narrow tokio features: now `["net", "io-util", "time", "rt", "macros", "sync"]` (down from
      `"full"`); `cargo build`/`cargo test` green. `rt` is still needed only by the unused
      `JoinHandleError` variant — dropping that variant would let `rt` go too (still present as of
      0.4.1).
- [x] `cargo publish --dry-run` clean; `exclude = ["certs/", "gen_self_signed_certs.sh"]` keeps the
      private keys and generator script out of the tarball (`cargo package --list` confirms no
      `.pem`/`.key`/`.crt` ship).
- [x] Declare an **MSRV**: `rust-version = "1.85.1"`, with a dedicated MSRV job in CI.
- [x] Verify the docs.rs build (it builds on a fixed toolchain) after the first publish.

## 5. Workstream — API stabilization (toward a stable `0.x` API)

Each item below is a breaking change. On `0.x` these stay relatively cheap, but the aim is to land
them, let the API **bake**, and then break only rarely — so do them deliberately and batch them per
release rather than dribbling breaks out continuously.

> **Note:** the items below record the API as settled at **0.4.0**. The unreleased `Environment`
> redesign (staged for `0.5.0`) has since superseded two of them — `Protocol<N>` → `Protocol<E:
> Environment>` and `simulate<P>` → `simulate<P, E>` with an environment factory — so where these
> entries say "now," read it as "as of 0.4.0."

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

- [x] **CSPRNG bounds for secret material.** The secret-generation APIs
      (`AdditiveSS::shares_from_secret`, `ShamirSS::shares_from_secret`, `FeldmanSS::shares_from_secret`)
      are now bound on `R: CryptoRng` (which, in rand 0.10, implies `RngCore`), so callers can't seed
      secret material from a predictable PRG. Lower-level, not-inherently-secret sampling
      (`Ring::random`, `Polynomial`/`Matrix`/`Vector::random`) deliberately still accepts any `Rng`.
- [ ] **Constant-time review** — _deliberately deferred while the library is research/prototyping
      (decided 2026-06-22); not a near-term priority._ secp256k1 field sampling uses
      `random_mod_vartime`; a future audit would check field/curve ops for data-dependent timing on
      secret inputs and either provide constant-time paths or document the absence precisely. The
      current "no side-channel guarantees" posture is already stated in `SECURITY.md`.
- [x] **Supply-chain hygiene.** `cargo-audit` (RUSTSEC advisories) runs in CI via a dedicated
      `Security audit` workflow (`.github/workflows/audit.yml`): on push/PR to `main` and a weekly
      cron, with `-D warnings` so unmaintained/yanked advisories also gate. Known-unfixable advisories
      are ignored in `.cargo/audit.toml` (currently only RUSTSEC-2023-0089, the target-conditional
      `atomic-polyfill` pulled in via postcard → heapless 0.7). `cargo-deny` (license/bans) not added.
- [ ] **Threat-model doc** stating what each primitive does and does not guarantee (ties to D-B).

## 7. Workstream — Networking & simulator correctness

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
  - [x] **Straggler / virtual-time regression test (sim).** Pins the property that a message from a
        slow party delivered *after* the receiver already passed its quorum does not distort the
        receiver's virtual time (delivery bumps `clock` in `deliver_next`, but it is inert once the
        party is done, and post-quorum synchronous work is stamped before any further delivery).
        `straggler_delivery_after_quorum_does_not_distort_collector_time` in `tests/simulator.rs`: a
        collector reaches quorum on fast senders while a straggler on a 20 s link is delivered (and
        bumps the collector's clock) only after the collector has finished; the collector's `Stop` is
        stamped at the quorum instant, strictly before the straggler's arrival (observed at a late
        receiver that keeps the run alive).
- [x] **Trace perspective bug.** _Resolved._ Send/receive events record the directed `Link` they
      occurred on, and `Event::Display` renders arrows from each party's own perspective by event
      direction (`sender -> recipient` outgoing, `recipient <- sender` incoming), so each party's
      trace reads from its own viewpoint. Guarded by the `trace_arrows_reflect_each_party_perspective`
      regression test (`tests/simulator.rs`). (The original canonical-`ChannelId` mechanism was
      superseded by the D10 `Link` unification below.)
- [x] **Nested protocol calls are now visible in traces.** `Protocol::execute` brackets every
      protocol invocation (top-level and inline sub-protocol) with `ProtocolBegin`/`ProtocolEnd`
      markers via no-op-by-default `Network::record_protocol_begin`/`record_protocol_end` hooks
      (overridden only by the simulator), and `SimulationTrace`'s `Display` renders the result as an
      indented brace-block tree that mirrors the call structure.
- [x] **D10 unification.** The duplicated `Link {recipient,sender}` (routing) and
      `ChannelId {local,remote}` (config/trace) are collapsed into one directed `Link {sender,
      recipient}` in `net::simulation::channel`; the dead `flip_end_points` is removed.
      `NetworkConfig::channel_config` is re-keyed to `Link`, enabling asymmetric up/down links (the
      delay is no longer canonicalized to an unordered pair). The same `Link` now serves routing,
      config, and the `Event` trace, and arrows are rendered from event direction (`sender ->
      recipient` / `recipient <- sender`).

## 8. Workstream — Quality gates & CI

CI now runs separate fmt / clippy / test / doc / MSRV jobs (the `module_inception` and
`needless_borrow` lints were cleared; `tests/simulator/` was flattened to a single
`tests/simulator.rs`), plus the dedicated `publish-dry-run` and `Security audit` workflows:

- [x] `cargo fmt --all --check`.
- [x] `cargo clippy --all-targets -- -D warnings` (green; pre-existing style lints cleared).
- [x] `cargo doc --no-deps -D warnings` in CI (keep intra-doc links honest).
- [x] `cargo test` on stable; a dedicated **MSRV** job builds on the pinned `rust-version = 1.85.1`
      (added in 0.3.0). _(The MSRV job runs `cargo build`, not the full test suite.)_
- [x] `cargo publish --dry-run` on tags — added in 0.4.0 (`.github/workflows/publish-dry-run.yml`,
      triggered on `v*` tags and manual dispatch). It also fails the job if any private-key/certificate
      material would be packaged.
- [x] `cargo-audit` in CI (`.github/workflows/audit.yml`, `-D warnings`); see §6. _(Coverage
      reporting and `cargo-deny` still optional/not added.)_

## 9. Workstream — Docs, examples & ecosystem

- [ ] **`examples/` directory** — _partially done._ Two examples exist: `simple_send_recv.rs` (a
      simulator run) and `additive_shr_secure_sum.rs` (a secret-sharing round-trip on the simulator).
      Still missing: (b) a runnable **real two-party TLS deployment** example (the binary sketched in
      the crate docs).
- [x] **`CHANGELOG.md`** (Keep a Changelog format) from 0.1.0 onward.
- [x] **`SECURITY.md`** added (status/posture + threat model & known limitations: variable-time
      sampling, non-CSPRNG `Rng` inputs, unaudited). Reporting channel is public GitHub issues for now
      (acceptable for a research tool); a private channel can be added if the posture changes.
- [ ] **`CONTRIBUTING.md`**.
- [x] Refresh `README.md`'s "Missing features" into a link to this roadmap; keep the security banner
      at the top. _(Done — the old checkbox list was replaced by the "Status and roadmap" section
      linking to this file; the two leftover specifics moved to §10 as "open README item"s.)_
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
| **0.5.0** (staged, unreleased) | _Composition & env redesign_         | The `Environment` trait redesign (`Protocol<E>`, `simulate<P, E>`, `GeneralEnv`), `Protocol::execute` + nesting-aware brace-block trace `Display`, CSPRNG bounds on the secret-generation APIs (§6), the `cargo-audit` CI workflow (§6/§8), the straggler/virtual-time regression test, and the D10 `Link` unification (§7). Breaking, so a minor bump per §2. Staged on `main`; not yet tagged. |
| **0.x**         | _Hardening & completeness_                         | Remaining §6 (constant-time review — deferred; threat-model doc), §9 docs (`CONTRIBUTING.md`, the real-TLS deployment example), and chosen §10 features.                                                                                                     |
| **0.x (stable)** | _API settled — steady state_                      | The §5 work has baked, public enums are `#[non_exhaustive]`, docs/examples are complete, and breaks are rare and deliberate. This is the intended steady state; `1.0` stays optional and unplanned (§2).                                                     |

---

## 12. Definition of a stable `0.x`

The bar for considering the `0.x` API "settled" — the steady state of §11, not a `1.0` gate:

- [ ] License recorded in `Cargo.toml` and consistent with `LICENSE`. _(Done — AGPL-3.0-or-later.)_
- [x] Security posture + threat model documented; `SECURITY.md` present; disclaimer prominent. _(Done
      — `SECURITY.md` states the unaudited posture, variable-time sampling, and side-channel
      limitations; the README banner and crate-doc disclaimer are prominent. A more detailed
      per-primitive threat-model doc (§6) remains optional.)_
- [x] Public API reviewed and deliberately settled: `Packet` reads return `Result`; public error enums
      `#[non_exhaustive]`; `Protocol` receiver and `Environment::clock` settled; prelude in place. _(Done
      across 0.2.0–0.4.0.)_
- [x] Secret-generation APIs require a CSPRNG (or the limitation is documented as a conscious choice).
      _(Done — `shares_from_secret` on additive/Shamir/Feldman is bound on `rand::CryptoRng`.)_
- [x] All §7 correctness loose ends closed. _(TLS `flush` (0.2.0), the real-TLS integration test
      (0.4.0), nested-call trace visibility, the straggler/virtual-time regression test, and the D10
      link unification are all done.)_
- [x] CI green on fmt, `clippy -D warnings`, `doc -D warnings`, tests on stable (build on MSRV
      1.85.1), `publish --dry-run`, and `cargo-audit`. _(All jobs present in `.github/workflows/`.)_
- [ ] `examples/` cover simulator + real deployment + secret sharing; `CHANGELOG.md` current.
      _(Partial — the simulator-run and secret-sharing examples exist and `CHANGELOG.md` is current; a
      real-deployment TLS example is still missing.)_
- [ ] `docs.rs` renders cleanly; README on-ramp is accurate end to end.

---

## 13. Deferred (later `0.x` or beyond)

- Constant-time / side-channel hardening (§6) — deliberately deferred while prototyping.
- Adversarial/reordering simulation harness (delay/drop/reorder deliveries) — a payoff of the
  explicit-blocking-state design (`Poll::Pending` = "party blocked on recv").
- Packet loss / retransmission modeling in the event loop.
- Compute-time / sender-side cost modeling in the virtual clock.
- Broader MPC protocol library on top of the typed-composition core.
