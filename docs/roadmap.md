# scl-rs Development Roadmap

**Date:** 2026-07-10

**Current version:** 0.10.0 (released 2026-07-10) — simulation hooks, typed protocol ids, and
post-hoc bandwidth profiling with flamegraph export. Before that: 0.9.1 (2026-07-08), an internal
waker-cleanup patch on top of 0.9.0 (released the same day), which completed the virtual-time
timeout primitive begun in 0.8.2 (2026-07-07). The MPC arithmetic layer landed in 0.8.0
(2026-07-06), with the simulator FIFO fix in 0.8.1 (same day). Before that: 0.7.1/0.7.0
(2026-06-30), 0.6.0 (2026-06-25), 0.5.2 (2026-06-23).

**0.10.0 contents:** a **simulation-hook module** — `TriggeredHook` moved out of `switchboard` into
its own `net::simulation::hook`, joined by the first built-in hook, `MetricHook`, which totals the
payload bytes each party puts on the wire (in aggregate and per sender; self-sends excluded — they
never leave the process). Plus the **`Protocol::name` → `Protocol::id` rename**, returning a `Copy`
`ProtocolId` newtype over `&'static str` (prelude-exported) instead of a bare `&'static str`, which
also becomes the type of the `protocol_name` field on the `ProtocolBegin`/`ProtocolEnd` trace
events. Both are breaking, so a minor bump per §2. On top of these, **post-hoc bandwidth
profiling**: `SimulationOutcome::bandwidth_tree_for` reconstructs a per-protocol call tree
(`ProtocolBandwidthTree` — self bytes per node, attributed to the innermost running protocol,
rebuilt recursively from each party's trace with no protocol instrumentation), and
`ProtocolBandwidthTree::write_folded` exports it in the folded-stacks flamegraph format (the §14
idea, shipped; rendering stays external — `examples/bandwidth_flamegraph.rs` and the multi-level
`examples/secure_stats_flamegraph.rs` show the `inferno` pipeline). See `CHANGELOG.md` `[0.10.0]`. **Still in development on top of the tree:** merging
repeated calls of the same protocol under the same parent, and a `SimulationMetrics` summary
(`SimulationOutcome::metrics()`) with a percentage-annotated tree `Display`.

**0.7.1 contents:** test- and documentation-only, no library API change. Completes the testing
plan: Tier 4 (all `tests/simulator.rs` protocols migrated to generic `impl<E: Environment>
Protocol<E>`, plus a run-to-run reproducibility test and a capability-carrying-environment test),
Tier 5 (real-network tests inline in `src/net/tcp.rs` — multi-party `recv_any` over mTLS and the
`ConnectionClosed`/`ConfigParse`/`InvalidPemFile` failure paths), and Tier 6 doctests
(per-constructor examples on `Packet`/`ShamirSS`/`FeldmanSS`). The Tier 4 reordering harness and
`cargo-deny` were both declined; the CSPRNG-bound doctest and constant-time work remain deferred.
See `CHANGELOG.md` `[0.7.1]`.

**0.7.0 contents:** Feldman VSS hardening — a new required `EllipticCurve::is_on_curve`
method, and `FeldmanSS::is_valid` now rejects off-curve dealer commitments before they reach
`scalar_mul` (closing the adversarial-dealer gap the testing plan's Tier 3 flagged), surfaced as
`ShareError::InvalidShare`. Adds the Tier-3 adversarial Feldman tests and point-level on-curve
regression tests. Also completes testing-plan **Tier 2**: a `proptest` property-based suite
(ring/field laws, Shamir subset-invariance over random `(secret, t, n)`, polynomial
evaluate-then-interpolate recovery, and `postcard` serialization round-trips for fields, curve
points, `ShamirSS`/`FeldmanSS`/`Packet`), with shared strategies in `tests/common/mod.rs`
(test-only). Advances testing-plan **Tier 3**: `Packet` read/`pop` rejection tests (the 0.4.0
`Result` API had no coverage) and a behavior change making `interpolate_polynomial_at` return errors
(`EmptyInterpolation`, `LengthMismatch`) instead of panicking on malformed input. Breaking (new
trait method), so a minor bump per §2. See `CHANGELOG.md` `[0.7.0]`.

**0.6.0 contents:** trace **element-type labels** — `SEND`/`RECV` lines now report a per-type
breakdown of a packet's contents (e.g. `(1024 bytes: 1 EC elem., 4 field elem.)`), driven by a new
`Abbreviate` trait (re-exported from the prelude) implemented by the built-in field/curve/poly/
vector/share types; `Packet::write_labeled`/`write_many_labeled`/`composition` and the
`content_count` field on the `SendData`/`ReceiveData` events. Plus two internal reorganizations: the
`net::simulation::runtime` → `net::simulation::simulator` module rename and the extraction of the
real-TLS backend into `net::tcp` (`TcpNetwork` still re-exported from `net`). Breaking (the module
rename, `Packet`'s representation + now-private `Packet::new`, the new `Event` fields, and the
removed legacy `Event::HasData`), so per the §2 policy it is a minor bump. See `CHANGELOG.md`.

**0.5.0 contents:** an environment-trait redesign (`Protocol<E: Environment>`, `simulate<P, E>` with
an environment factory, `GeneralEnv`), `Protocol::execute` with a nesting-aware brace-block trace
`Display`, CSPRNG bounds on the secret-generation APIs, a `cargo-audit` CI workflow, a
straggler/virtual-time regression test, the D10 `Link` unification, and the `send_many` scatter
primitive. These are breaking, so per the §2 policy it is a minor bump. See `CHANGELOG.md`.

**Versioning stance:** scl-rs stays on **`0.x` indefinitely**. `1.0` is **not a planned milestone** —
the "unaudited / not for production" posture is carried by the security disclaimer (not the version
number), and the API has no downstream usage yet to justify freezing it. See §2.

**Goal:** evolve toward a **stable, well-baked `0.x` API** — one that settles across releases and
breaks only rarely — while keeping the library useful for prototyping MPC protocols.

This document is a living plan. It captures where the library is today, the work grouped into themed
workstreams, a suggested version sequence, and a **Definition of a stable `0.x`** (§13).

---

## 1. Current state (honest snapshot)

**What exists and works** (`cargo build`, `cargo test`, `cargo doc -D warnings` all clean; ~5.6 kLOC):

- **`math`** — `Ring` and `FiniteField<const LIMBS>` traits; Mersenne‑61 field; secp256k1 base
  field, scalar field, and curve; rings, polynomials (Lagrange interpolation), matrices, vectors,
  NAF. Reasonable unit-test coverage.
- **`ss`** — additive sharing, Shamir, Feldman VSS, with a generic `ShareError<T: Ring>`.
- **`net`** — real TLS point-to-point networking (`TcpNetwork` over `tokio-rustls`) **and** a
  single-threaded deterministic discrete-event simulator (`SimNetwork` + `Switchboard` + virtual
  clock), both behind one `Network` trait. The simulator records a per-party event trace and exposes
  a `TriggeredHook` extension point (`net::simulation::hook`) for observing or steering a run.
- **`protocol`** — a `Protocol<E: Environment>` trait with a typed `Output`; protocols compose by
  calling one another through `Protocol::execute` (which brackets each call with trace markers).
  `Environment` is the ambient-context seam (`GeneralEnv` is the default), and `simulate<P, E>` runs
  protocols deterministically and returns typed outputs + nesting-aware event traces.

**Published and iterating** (see §4): releases `0.2.0`–`0.10.0` have shipped to crates.io. `Cargo.toml`
has `license`, `description`, `keywords`, `categories`, `repository`, `readme`; tokio features are
narrowed; `certs/` and the generator script are excluded; `cargo publish --dry-run` passes in CI; MSRV
is pinned at 1.85.1; and the security disclaimer + `SECURITY.md` are in place. The `Environment`
redesign that was staged for the next `0.x` shipped in `0.5.0` (see §12).

The testing plan (Tiers 1–6) is **complete as of 0.7.1** — every tier is shipped or a recorded
deferral/decline (see §10) — so testing is no longer an open workstream.

**Tier 1 of the §11 MPC protocol library is complete as of `0.11.0`.** The crate can now *compute on
shares*, not merely share and reconstruct them. `0.8.0` shipped the `LinearShare` arithmetic seam and
the passive deal/open protocols; `0.11.0` shipped the multiplication half — the DN07 protocols
(`protocol::passive_shamir`): shared-randomness generation via Vandermonde extraction (`Random`,
`Double-Random`), a king-routed batched open, multiplication-triple generation, and Beaver
multiplication, with a worked arithmetic-circuit example (`examples/secure_covariance.rs`). The one
Tier-1 item still open is **error-detecting reconstruction**, which is a *malicious-model* upgrade
rather than a missing arithmetic capability.

**What remains** is therefore mostly (a) _productization_ — finishing the §6 hardening (constant-time
review — deferred; threat-model doc) and chosen §10 features (`CONTRIBUTING.md` is deferred until
there are outside contributors — see §9/§14) — and (b) the **higher tiers of §11**: the rest of Tier 2
(`rand_bit`, coin-tossing, broadcast), Tier 3 comparisons, and the malicious-model work (active
deal/open variants, error-detecting reconstruction). All of it is work that improves the `0.x` line,
not a checklist gating a `1.0`.

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

> **Note:** the items below record the API as settled at **0.4.0**. The `Environment` redesign
> shipped in **`0.5.0`** has since superseded two of them — `Protocol<N>` → `Protocol<E:
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
      _(The `Send` requirement still holds and `Network: Send` remains, but as of **0.13.0** it is
      carried by `-> impl Future + Send` in the trait definitions rather than by `#[async_trait]`;
      see the entry below.)_
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
      — without `P: Clone` or per-party boilerplate. (`src/net/simulation/simulator.rs`.)
- [x] **Re-exports / prelude — added in 0.4.0.** A `prelude` module now re-exports the common path so
      users aren't deep-pathing. _(Extended in 0.10.0: `ProtocolId` joins `Protocol` in
      the prelude, so implementing the trait doesn't need a second import.)_
- [x] **`Protocol::name` → `Protocol::id`, returning `ProtocolId` (0.10.0).** The identifier a
      protocol reports for tracing is now a `Copy`, allocation-free newtype over `&'static str`
      (`protocol::ProtocolId`, built with `ProtocolId::from`, rendered through `Display`) rather than
      a bare `&'static str`. The rename separates a protocol's *trace identity* from any
      human-readable description, and the newtype gives the id a place to gain structure later
      (a version, a parameterized instance name) without touching the `Network` trace API again.
      `Network::record_protocol_begin`/`record_protocol_end` and the `ProtocolBegin`/`ProtocolEnd`
      events carry `ProtocolId` accordingly. Breaking (every `impl Protocol` updates), so a minor
      bump per §2.
- [x] **`async-trait` dependency dropped (0.13.0).** `Protocol` and `Network` now use native
      async-fn-in-trait, stable since Rust 1.75 and comfortably inside the `1.85.1` MSRV. Their async
      methods are declared `fn … -> impl Future<Output = …> + Send` rather than `async fn`: a bare
      `async fn` in a public trait leaves the future's auto traits unspecified, so the explicit
      `+ Send` is what preserves the guarantee `#[async_trait]` used to provide (and what the
      `Network: Send` entry above depends on). Implementors write a plain `async fn` and delete the
      attribute. Breaking (every `impl Protocol`/`impl Network` updates), so a minor bump per §2.
      Gives up `dyn Protocol`/`dyn Network`, which nothing used; boxing a protocol's *future* still
      works, so the simulator's executor is unaffected. Also removes a proc-macro from the build
      graph and the per-call `Pin<Box<dyn Future>>` allocation, and un-hides clippy inside trait
      method bodies (clippy skips macro-expanded code — two latent lints surfaced and were fixed).
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
- [x] **Feldman commitment on-curve validation (shipped in 0.7.0).** `FeldmanSS::is_valid` now
      rejects dealer-supplied commitments that are not on the curve — via a new required
      `EllipticCurve::is_on_curve` method (implemented for `Secp256k1`; the point at infinity
      short-circuits to `true` rather than panicking through `to_affine`) — *before* they feed into
      `scalar_mul`, closing the adversarial-dealer gap the testing plan's Tier 3 flagged. A tampered
      or off-curve share surfaces as `ShareError::InvalidShare`. Guarded by adversarial tests
      (off-curve commitment, tampered share, wrong commitment-vector length, length mismatch) and
      point-level on-curve regression tests. Breaking (new trait method) → minor bump per §2.
- [ ] **Constant-time review** — _deliberately deferred while the library is research/prototyping
      (decided 2026-06-22); not a near-term priority._ secp256k1 field sampling uses
      `random_mod_vartime`; a future audit would check field/curve ops for data-dependent timing on
      secret inputs and either provide constant-time paths or document the absence precisely. The
      current "no side-channel guarantees" posture is already stated in `SECURITY.md`.
- [x] **Supply-chain hygiene.** `cargo-audit` (RUSTSEC advisories) runs in CI via a dedicated
      `Security audit` workflow (`.github/workflows/audit.yml`): on push/PR to `main` and a weekly
      cron, with `-D warnings` so unmaintained/yanked advisories also gate. Known-unfixable advisories
      are ignored in `.cargo/audit.toml` (currently only RUSTSEC-2023-0089, the target-conditional
      `atomic-polyfill` pulled in via postcard → heapless 0.7). `cargo-deny` (license/bans) was
      considered and **declined** (2026-06-30): its advisory check duplicates `cargo-audit`, and the
      license/bans/sources surface does not justify a second tool at this dependency scale.
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
- [x] **Simulation hooks have their own module, and a first built-in (0.10.0).**
      `TriggeredHook` moved from `net::simulation::switchboard` to a new `net::simulation::hook`
      (breaking: the import path changed; the trait and the switchboard's dispatch are unchanged), so
      the extension point no longer lives inside the message router it observes. `MetricHook` is the
      first built-in implementation: it triggers on `SendData` and totals the payload **bytes** each
      party puts on the wire, in aggregate and per sender — the communication-cost measurement an MPC
      protocol is usually benchmarked on. Guarded by `metric_hook_fires_on_matching_event` in
      `tests/simulator.rs`. _Future hooks land here too: a latency/round counter, and the
      delay/drop/reorder steering hook sketched in §14._
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
      reporting remains optional; `cargo-deny` was **declined** — see §6/§10.)_

## 9. Workstream — Docs, examples & ecosystem

- [x] **`examples/` directory** — _done._ Three examples exist: `simple_send_recv.rs` (a simulator
      run), `additive_shr_secure_sum.rs` (a secret-sharing round-trip on the simulator), and
      `real_tls_send_recv.rs` (the same `SendRecvProtocol` run over a **real two-party mTLS
      deployment**, with committed `config_p0.json`/`config_p1.json` configs and run instructions in
      its module docs). The simulator and real-network backends are now both demonstrated end to end.
- [x] **`CHANGELOG.md`** (Keep a Changelog format) from 0.1.0 onward.
- [x] **`SECURITY.md`** added (status/posture + threat model & known limitations: variable-time
      sampling, non-CSPRNG `Rng` inputs, unaudited). Reporting channel is public GitHub issues for now
      (acceptable for a research tool); a private channel can be added if the posture changes.
- [ ] **`CONTRIBUTING.md`** — _deliberately deferred (decided 2026-06-22) until the project attracts
      contributors beyond the sole maintainer._ A contribution guide has no audience while there is
      one author; it will be written if/when outside contributors appear. See §14.
- [x] Refresh `README.md`'s "Missing features" into a link to this roadmap; keep the security banner
      at the top. _(Done — the old checkbox list was replaced by the "Status and roadmap" section
      linking to this file; the two leftover specifics moved to §10 as "open README item"s.)_
- [x] **Rename `runtime.rs` → `simulator.rs` — done (0.6.0).** The simulator module is now
      `net::simulation::simulator` (`simulate`/`SimulationOutcome` re-exported through the prelude, so
      prelude users are unaffected; deep-path users update `net::simulation::runtime` →
      `net::simulation::simulator`). Breaking, so a minor bump per §2. Bundled with the `net::tcp`
      split below.
- [x] **`TcpNetwork` extracted to `net::tcp` — done (0.6.0).** The real-TLS backend
      (`TcpNetwork` + the private `PeerWriter`/`PacketStream`) moved out of the 876-LOC `net/mod.rs`
      into its own `net::tcp` module, mirroring `net::simulation`. `net/mod.rs` now holds just the
      shared contract (`PartyId`, `Packet`, `NetworkError`, `NetworkConfig`, `Network`). `TcpNetwork`
      is re-exported from `net`, so `net::TcpNetwork` and the prelude are unchanged — no public API
      change.

## 10. Workstream — Feature completeness (scope to taste)

These are not strictly required, but shape how "complete" the stable `0.x` surface feels.

- [ ] **Arbitrary prime-`p` field** (open README item): a general `F_p` instead of only the
      hand-written Mersenne‑61 / secp256k1 fields.
- [ ] **Test-coverage gap** (open README item): "write missing tests for all functionalities."
      _Progress: `matrix`/`shamir`/`vector`/`naf` landed in 0.5.2; `ss::feldman` adversarial +
      point-level on-curve tests shipped in 0.7.0 (§6); testing-plan **Tier 2** complete
      (shipped in 0.7.0) — `proptest` ring/field laws, Shamir subset-invariance, polynomial
      evaluate-then-interpolate recovery, and `postcard` serialization round-trips for
      fields/curve points/`ShamirSS`/`FeldmanSS`/`Packet`, with shared strategies in
      `tests/common/mod.rs`. Testing-plan **Tier 3** (shipped in 0.7.0) — Feldman off-curve rejection
      (§6), `Packet` read/`pop` rejection tests, and `interpolate_polynomial_at` now erroring (rather
      than panicking) on empty/length-mismatch input._ The remaining Tier 3 item (the CSPRNG-bound doc
      test) is **deferred** alongside the broader CSPRNG/constant-time hardening. Testing-plan **Tier 4**
      (shipped in 0.7.1): the `tests/simulator.rs` protocols are migrated to generic
      `impl<E: Environment> Protocol<E>`, plus a run-to-run reproducibility test and a
      capability-carrying-environment test; the adversarial reordering harness was declined (below).
      Testing-plan **Tier 5** (shipped in 0.7.1): real-network tests landed inline in
      `src/net/tcp.rs` — multi-party (`n > 2`) `recv_any` over real mTLS, plus failure paths for
      `ConnectionClosed` (closed mid-receive), `ConfigParse` (malformed config JSON), and
      `InvalidPemFile` (unloadable PEM). Testing-plan **Tier 6** (shipped in 0.7.1): per-constructor doctests
      landed on `Packet::empty` and `ShamirSS`/`FeldmanSS`'s `new`/`shares_from_secret` (the MSRV job
      shipped earlier in 0.3.0). `cargo-deny` was considered and **declined** (2026-06-30): its
      advisory check duplicates the wired `cargo-audit`, and its extra license/bans/sources checks
      don't justify a second tool for this dependency set. The Tier 4 adversarial reordering harness
      was **declined** (2026-06-30) and left out of the test suite, which closes out the testing plan:
      all remaining items are shipped or deliberately deferred/declined.
- [ ] Any additional MPC facilities you want in the stable surface (e.g. opening/reconstruction
      helpers, a Beaver-triple/multiplication example to showcase typed composition end-to-end).
      **This bullet is now expanded into a full workstream in §11** — the proposed MPC protocol
      library that turns the crate from "a secret-sharing crate" into "an MPC crate."

---

## 11. Workstream — MPC protocol library (compute-on-shares)

**Motivation (honest snapshot).** As of 0.7.1 the library has the two halves of an MPC toolkit but
not the bridge between them: it has the **primitives** (`math` fields/curves; `ss` additive/Shamir/
Feldman sharing) and the **typed composition core** (`Protocol<E: Environment>`, `simulate`,
`Protocol::execute` tracing, and the `recv_any`/`send_many` collective primitives), but almost no
**protocol layer** built on top. You can *share* a secret and *reconstruct* it, but you cannot yet
*compute on shares*: share addition is a local operation with no wrapper, and there is no opening
protocol, no multiplication, no shared-randomness generation, and no broadcast. This workstream is
the "broader MPC protocol library on top of the typed-composition core" flagged in §14, made
concrete. It is scoped to the crate's current posture — **honest-but-curious, honest-majority,
no side-channel guarantees** (see D-B/§6) — and each item states where it would need more to reach
malicious security so the boundary stays honest.

The tiers below are ordered by **leverage**: Tier 1 is the foundation every later tier depends on,
and nothing in Tiers 2–4 should land before it. Individual protocols are new *additive* surface
(new modules/types), so most ship as **patch or minor** releases per §2 rather than breaking ones;
the exceptions are called out.

### 11.1 Tier 1 — the arithmetic layer (unlocks everything else)

The single highest-leverage slice. Self-contained, directly satisfies the two original §10 bullets,
exercises `send_many`/`recv_any`/typed `Output` end to end, and is a prerequisite for every later
tier. Suggested home: a new `mpc` module (e.g. `src/mpc/`), keeping `ss` as the pure-sharing layer.

- [x] **The `LinearShare` trait — the arithmetic seam (DONE).** _Design settled as a **trait on the
      existing share types**, not a `Shared<F>` wrapper._ The originally-sketched wrapper was dropped:
      its main justification (carrying the party's evaluation point) evaporated once we saw that the
      local operations don't need the point at all — it is only needed at *open* time, where the
      protocol derives it from `network.local_party()`. So `ss::LinearShare` sits directly on
      `ShamirSS`/`AdditiveSS` (and any future scheme — replicated, packed), giving generic protocols
      one seam over every linear scheme. Shape:
  - **Local operators as supertrait bounds** (all communication-free): `Add<&Self>`/`Sub<&Self>`/`Neg`
    (share-wise), and `Add<&Value>`/`Sub<&Value>`/`Mul<&Value>` (public constant/scalar). Implemented
    on both `ShamirSS` and `AdditiveSS`; share-ops `debug_assert!` compatible metadata (equal Shamir
    degree; same additive holder). **Share×share is deliberately absent** — it is non-linear (see the
    Beaver item).
  - **`encode_party(PartyId) -> Value`** is the canonical party→field map, and it lives *on the trait*
    (invariants: injective, never `Ring::ZERO`), not on `FiniteField`. Shamir implements it with a
    **local `F: From<u64>` bound**, so `FiniteField` stays clean for a future polynomial Galois field
    (whose injective embedding is bit-packing, not `From<u64>`-as-arithmetic-value). Additive never
    consults it.
  - **`shares_from_secret` / `secret_from_shares`** are on the trait (positional `shares[i] ↔
    parties[i]`; reconstruction returns `Result<Value, ShareError>`), delegating to the inherent
    per-scheme methods.
  - **Additive public-constant add/sub are party-dependent** (only one party may absorb `c`, else you
    get `x + n·c`). Resolved by giving `AdditiveSS` a `{ party, is_leader }` — the **leader = smallest
    party id**, decided at deal time and stamped on every share — so `Add<&Value>` is correct for any
    party numbering (Shamir is symmetric: every party adds `c`). Required `PartyId` to derive
    `Serialize`/`Deserialize` (share now carries it) and `Ord` (to pick the min).
  - **Two trade-offs of the original no-RNG / no-threshold trait signatures — both resolved in
    `0.11.0`.** As first shipped, trait-level `shares_from_secret` drew from `rand::rng()` (so it was
    **not seed-reproducible**) and Shamir's trait-level dealing was **full-threshold (`n`-of-`n`)`**
    (so trait-dealt Shamir values were **not Beaver-multipliable**: product degree `2(n-1) > n-1`).
    `0.11.0` fixed both by making the trait threshold- and RNG-aware — `shares_from_secret(secret,
    parties, threshold, rng)`, with an associated `Threshold` type (`usize` polynomial degree for
    Shamir, `()` for additive, whose threshold is structural). Dealing now draws from the
    environment's session RNG through the new `RandEnvironment`, so seeded runs are reproducible end
    to end, and Shamir can be dealt at any degree `t < n` — which is what made the DN07
    multiplication layer possible at all. _Breaking; shipped in `0.11.0`._
  - Shipped in `src/ss/mod.rs` (trait) + `shamir.rs`/`additive.rs` (impls), with the two examples and
    `tests/additive.rs` migrated to the reshaped additive deal signature. Builds green (`cargo test`,
    doctests, `clippy --all-targets`). _Non-breaking library-API-wise except the `AdditiveSS`
    representation + its inherent `shares_from_secret(count)` → `(parties)` change and the `PartyId`
    derives — batch into the next minor per §2._
- [x] **Deal / `open` protocols (passive-adversary versions) — DONE.** Shipped in
      `protocol::share` (the `protocol` module became a directory to host them), all generic over
      `S: LinearShare` and all **explicitly passive (semi-honest)** — every party follows the
      protocol and always sends, so blocking receives are safe and the message pattern is exactly
      balanced (no leftover packets to poison a later `recv_any`). The `Passive*` names carry the
      model:
  - `PassiveDealShr` — a designated dealer splits a secret (`LinearShare::shares_from_secret`) and
    scatters one share per receiver via `send_many`; receivers are constructed *without* a secret
    (dealer-only input behind an `Option`, misuse surfaced as `protocol::Error::Input`). The
    dealer must list itself among its receivers.
  - `PassiveOpenShr` (the `open_to_all` sketch) — every party `send_many`s its share to all others
    and collects one share from *every* peer via `recv_any` (arrival order, paired with sender ids
    for `encode_party`), then reconstructs. The earlier `shares_needed`/`t+1` parameter was
    dropped: waiting for a quorum leaves the stragglers' packets queued (a composition hazard),
    and under the passive assumption all `n` shares always arrive.
  - `PassiveOpenToParty` (the `open_to` sketch) — reveal only to a single output party; the
    receiver reconstructs (`Some(secret)`), everyone else sends one message and outputs `None`.
      Also added the `protocol::Error::{Share, Input}` variants — `Share` boxes a type-erased
      `ShareError<T>` so the `Protocol` trait stays ring-independent. _Non-breaking (new
      protocols)._
  - [x] `open_many` — **DONE (0.11.0)**, as `passive_shamir::open_king::BatchedPassiveOpenToKing`:
    a `Vec` of shares opened in **two rounds regardless of batch size**, one `Packet` per peer
    carrying every value (`write_many_labeled`, read back positionally). It routes through a
    designated *king* rather than opening all-to-all — `O(n)` messages instead of `O(n²)` — which is
    what keeps the round count of a circuit independent of its gate count. An all-to-all
    `PassiveOpenShr`-style batched variant remains possible but has no caller today.
- [x] **Malicious-model receives, piece (1): the `recv_timeout` primitive — DONE (0.8.2).** The
      `Passive*` protocols block forever on a party that crashes or withholds its share — sound
      only under the passive assumption. The network half of lifting that shipped in 0.8.2:
      **`Network::recv_from_with_timeout(party, timeout)`**, which bounds how long a receiver waits
      and *identifies* the silent party via a new `NetworkError::Timeout(PartyId)`. This is the §14
      "in-protocol timeout / deadline primitive (**virtual-time**)" item: under the deterministic
      simulator the deadline is a **virtual-clock timer event scheduled on the switchboard** (the
      deadline is captured on first poll as `recipient clock + timeout`; a packet arriving exactly
      at the deadline wins the race, matching `tokio::time::timeout` semantics), so simulated and
      real deployments keep identical behavior — `TcpNetwork` maps it to `tokio::time::timeout`.
      Regression tests in `tests/recv_timeout.rs` pin both outcomes (silent party → `Timeout` with
      the culprit's id; prompt sender → packet). The **`recv_any_with_timeout`** variant shipped
      in 0.9.0 (same timer design, one timer per call; `Timeout(None)`, since no single party can
      be blamed), completing the primitive. _The "default method or breaking change?"
      question resolved itself the hard way: it shipped as a **required** trait method in patch
      0.8.2 — technically breaking for external `Network` implementors, accepted as a deliberate
      one-time exception to the §2 convention._
- [ ] **Malicious-model receives, piece (2): active variants of the deal/open protocols**, built on
      the 0.8.2 `recv_from_with_timeout` (abort-with-culprit on timeout; combine with the
      error-detecting reconstruction item below for tampered — not just missing — shares).
      _Non-breaking (new protocols)._
- [ ] **Error-detecting / robust reconstruction (Shamir).** A stricter `open` variant that uses the
      code's redundancy: reconstruct from `t+1` shares, then check the remaining shares lie on the same
      degree-`t` polynomial and surface `ShareError::InvalidShare` (reusing the enum) if not. This is
      the honest-majority analogue of the Feldman on-curve hardening shipped in 0.7.0 — it upgrades
      *opening* from honest-but-curious to **cheater-detecting**. Full Reed–Solomon error *correction*
      (Berlekamp–Welch) is a larger follow-on and can be a separate item. _Non-breaking._
- [x] **Beaver-triple multiplication — the flagship demo. DONE (0.11.0).** Shipped in
      `protocol::passive_shamir`, and it **overshot the plan**: the sketch below budgeted a
      *trusted-dealer* `TripleSource` as a placeholder, but the triples are generated by a real
      distributed offline phase — the DN07 protocols of Damgård & Nielsen (CRYPTO 2007) — so no party
      or dealer ever knows `a`, `b` or `c`. The `TripleSource` trait was therefore never needed:
  1. **The preprocessing phase.** `PassiveRandShr` (`Random`) and `PassiveRandDoubleShr`
     (`Double-Random`) have every party deal one sharing of a value it samples itself, then compress
     the `n` dealt sharings into `n - t` that are uniformly random even if `t` of the dealers rigged
     their inputs — a transposed-Vandermonde extraction, which is what makes shared randomness cost
     `O(n)` per party rather than `O(n²)`. `PassiveTriple` turns those into triples: mask the local
     (degree-`2t`) product with the degree-`2t` half of a `DoubleShare`, open it, subtract the
     degree-`t` half. This subsumes the Tier-2 `rand_shared()` item below.
  2. **The online multiply protocol** — `PassiveShamirMul`: consume one triple, open `d = x − a` and
     `e = y − b` (a single batched round through `BatchedPassiveOpenToKing`), then recombine locally,
     `[x·y] = [a·b] + d·[b] + e·[a] + d·e`. The result is degree `t`, so it feeds the next
     multiplication. A whole batch of products costs **one** round, so a circuit's round count tracks
     its multiplicative *depth*, not its gate count.
      Shipped with the worked example the roadmap asked for: `examples/secure_covariance.rs` — the
      covariance between two parties' private datasets, an arithmetic circuit whose multiplications
      are *unavoidable* (the operands live on different machines, so no party can pre-compute a
      product locally the way `secure_stats_flamegraph.rs` sidesteps squaring). It runs on the
      simulator and exports a bandwidth flamegraph (`docs/cov_bandwidth.svg`, reproduced in the
      README) that attributes 57% of the 3,633 bytes to preprocessing against 25% for the online
      multiplication — most of the traffic is input-independent and can leave the critical path.
      Regression tests in `tests/passive_shamir.rs`. _Non-breaking (new protocols); the enabling
      `LinearShare` threshold/RNG change above was the breaking part._

**Recommended first release (a coherent `0.8.0`):** the two shipped Tier-1 items — the
`LinearShare` trait (with the local operators) and the passive deal/open protocols. That is a
self-contained, honest slice: the arithmetic seam plus the interactive ends that let a protocol
share, compute linearly, and open. The Tier-1 remainder — `open_many`, error-detecting
reconstruction, and trusted-dealer Beaver `mul` with its worked example — is deliberately **not**
in `0.8.0` and ships in a later `0.x` slice, together with (or after) the malicious-model
`recv_timeout` work above _(the network primitive half of which has since shipped in 0.8.2)_.

### 11.2 Tier 2 — randomness & agreement

Prerequisites for most higher protocols; all build directly on Tier 1.

- [x] **Shared-randomness protocols — `rand_shared` DONE (0.11.0); `rand_bit` still open.**
      `PassiveRandShr` (§11.1) delivers this and improves on the sketch: rather than summing one
      contribution per party to get *one* shared random value, DN07's Vandermonde extraction yields
      `n - t` of them from the same single dealing round, so the amortized cost is `O(n)` per party
      instead of `O(n²)`. Randomness is drawn from the environment's session RNG via
      `RandEnvironment` (bound on `CryptoRng`, per the §6 posture), so seeded runs stay reproducible.
      Still open: **`rand_bit()`** — a shared value guaranteed to be `0`/`1` (e.g. via the
      square-root trick), the building block for the Tier-3 comparisons. _Non-breaking._
- [ ] **Coin-tossing.** A public unbiased random value via commit-then-open (needs the Tier-3
      commitment item, or a hash commitment inline). Useful for Fiat–Shamir-style challenges and for
      seeding. _Non-breaking._
- [ ] **PRSS (pseudo-random secret sharing).** Replica-seeded, **non-interactive** shared randomness:
      parties pre-share seeds once, then derive unbounded shared random values locally with a CSPRNG.
      A very natural fit given Shamir sharing plus the existing CSPRNG posture, and it makes Tier-2
      randomness essentially free after setup. _Non-breaking._
- [ ] **Broadcast primitives on `recv_any`.** `recv_any` was added in 0.3.0/0.4.0 precisely as "the
      building block for quorum-based protocols"; this cashes it in. Start with **echo broadcast**
      (round-based, honest-majority) and a simple **reliable broadcast** (Bracha-style: send → echo →
      ready, deliver on quorum), both as `Protocol<E>` impls that wait for the first `k`-of-`n` via
      `recv_any` and never block on silent parties. These are also the natural first consumer of the
      §14 virtual-time timeout primitive (complete since 0.9.0: `recv_from_with_timeout` and
      `recv_any_with_timeout`). _Non-breaking._

### 11.3 Tier 3 — richer computation

- [ ] **Linear algebra over shares.** Shared inner product and matrix×vector / matrix×matrix multiply,
      reusing the existing `math::matrix` / `math::vector` types with Tier-1 `Shared<F>` entries and
      the Beaver `mul` (batch all the products into one preprocessing draw + one opening round). Cheap
      once Tier 1 exists; a good showcase of amortized `open_many`. _Non-breaking._
- [ ] **Bit-decomposition and comparison** (`<`, `≤`, `==`, `is_zero`, `msb`). The gateway from
      arithmetic MPC to non-arithmetic MPC (sorting, selection, thresholding). This is a **larger
      effort** — it needs shared random bits (Tier 2), bitwise sub-protocols, and careful field-size
      handling — so it is a milestone, not a quick win. Flag it as its own release. _Non-breaking._
- [ ] **Commitment schemes.** A hash-based commitment (`commit`/`open`) and a **Pedersen commitment**
      over the existing secp256k1 curve (`g^m · h^r`). Low cost given `math::ec` is already present,
      and it underpins coin-tossing (Tier 2) and any move toward malicious security. _Non-breaking._

### 11.4 Tier 4 — more sharing schemes & building blocks

- [ ] **Replicated secret sharing (3-party, honest-majority).** A different sharing flavor where local
      multiplication is cheap; broadens how "complete" the stable surface feels and pairs well with the
      Tier-1 scheme-generic `Shared` trait. _Non-breaking (new scheme in `ss`/`mpc`)._
- [ ] **Packed Shamir sharing.** Amortized, SIMD-style sharing (several secrets per polynomial) for
      throughput. Slots into the same `Shared` abstraction. _Non-breaking._
- [ ] **Arbitrary prime-`p` field** (also the open §10 item). A general `F_p` instead of only the
      hand-written Mersenne-61 / secp256k1 fields, so all of the above can run over a caller-chosen
      modulus. This is the one item likely to interact with existing trait bounds
      (`FiniteField<const LIMBS>`), so scope its API impact deliberately — possibly a **minor** bump.
- [x] **Virtual-time timeout / deadline primitive — DONE (0.8.2 + 0.9.0)** (also tracked in §14).
      Shipped as `Network::recv_from_with_timeout` (0.8.2) and `Network::recv_any_with_timeout`
      (0.9.0): a virtual-time timer event on the switchboard that races the message
      deterministically on the simulator, mapped to `tokio::time::timeout` on `TcpNetwork`. See
      the §11.1 item for details.

### 11.5 Explicitly out of scope (for now)

Called out so the boundary stays honest — each is a large workstream that would pull the crate well
past its current honest-but-curious prototyping posture, and none should be started without a
deliberate decision:

- **Oblivious transfer (OT) / OT extension and OLE** — the real offline phase behind Beaver triples;
  Tier 1 uses a trusted dealer precisely to avoid this.
- **Garbled circuits / Yao's protocol** — a different MPC paradigm from the arithmetic-sharing line
  the crate is built on.
- **Malicious / dishonest-majority security** (MACs à la SPDZ, zero-knowledge proofs of correct
  behavior, verifiable secret sharing beyond Feldman). The error-detecting reconstruction (Tier 1)
  and commitments (Tier 3) are honest steps in this direction, but full malicious security is its own
  multi-release program and a change of threat model (D-B).

### 11.6 Dependency summary

```
Tier 1  LinearShare  ──►  passive deal/open  ──►  [0.8.0 slice]
   │                         │
   │                         ▼
   │       open_many / error-detecting open ──► Beaver mul  ──► [future 0.x slice]
   │                         │                    │
   ▼                         ▼                    ▼
Tier 2  rand_shared/bit ─► coin-toss     linear algebra (Tier 3)
   │        PRSS             │
   ▼                         ▼
Tier 2  broadcast (recv_any) ─────────────► needs §14 timeout ✅ [0.9.0]
Tier 3  commitments ─► coin-toss ; bit-decomp ─► comparison (needs rand_bit)
Tier 4  replicated / packed SS ─► reuse Shared trait ; F_p ─► general modulus
```

---

## 12. Suggested release sequence

Ship early and often on `0.x`; let the API bake and then break only rarely. There is no `1.0` row — a
stable, patch-mostly `0.x` is the intended terminal state (§2).

| Version         | Theme                                              | Contents                                                                                                                                                                                                                                                    |
| --------------- | -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **0.2.0**       | _Publishable & honest_ ✅ **PUBLISHED 2026-06-16** | §4 metadata/license/tokio features, the §7 `flush` fix, `SECURITY.md`, compiled doctests, `Network: Send`, factory `simulate`, §8 CI (fmt/clippy/test/doc), and corrected real-network docs. **First crates.io release**, tagged `v0.2.0`, an early `0.x` release. |
| **0.3.0**       | _Correct & clean_ ✅ **PUBLISHED 2026-06-17**      | Mutual TLS (mTLS) — wire-incompatible with 0.2.0; MSRV 1.85.1 + MSRV CI job; typed `serde` config parsing (`deny_unknown_fields`, `base_port` range check); `channel_id` perspective bug resolved + regression test; mTLS handshake tests (positive + negative); `Network::recv_any` **simulator-only** (quorum primitive). Tagged `v0.3.0`.                  |
| **0.4.0**       | _API stabilization_ ✅ **PUBLISHED 2026-06-19**    | §5 in full (Packet `Result` API, error sweep incl. `NetworkConfig::new` → crate error, `Protocol` consumes `self`, `Environment` clock, prelude, naming/visibility audit). Plus the §7 `TcpNetwork::recv_any` implementation (cancel-safe `FramedRead` + `StreamMap` multiplexing) and the `tls_public_api_correctness` real-TLS socket integration test, and the §8 `publish-dry-run` tag workflow. Tagged `v0.4.0`. |
| **0.5.0**       | _Composition & env redesign_ ✅ **PUBLISHED 2026-06-22** | The `Environment` trait redesign (`Protocol<E>`, `simulate<P, E>`, `GeneralEnv`), `Protocol::execute` + nesting-aware brace-block trace `Display`, CSPRNG bounds on the secret-generation APIs (§6), the `cargo-audit` CI workflow (§6/§8), the straggler/virtual-time regression test, the D10 `Link` unification (§7), and the `send_many` scatter primitive. Breaking, so a minor bump per §2. Tagged `v0.5.0`. |
| **0.5.1**       | _Docs patch_ ✅ **PUBLISHED 2026-06-22**            | Doc-snippet fixes on top of 0.5.0 (no API change). Tagged `v0.5.1`.                                                                                                                                                                                          |
| **0.5.2**       | _Correctness patch_ ✅ **PUBLISHED 2026-06-23**     | Fixed a `Matrix` non-square indexing bug (`get`/`get_mut`/matrix×matrix and matrix×vector `mul` used the row count as the row stride; `get`/`get_mut` now bounds-check both axes), derived `Clone` for `FeldmanSS`, and expanded the test suite (Shamir, NAF, matrix, vector — the matrix tests are what caught the bug). Tagged `v0.5.2`. |
| **0.6.0**       | _Trace element labels & reorg_ ✅ **RELEASED 2026-06-25** | Trace **element-type labels**: `SEND`/`RECV` lines report a per-type breakdown (`(1024 bytes: 1 EC elem., 4 field elem.)`) via a new `Abbreviate` trait (prelude-exported; implemented by the built-in field/curve/poly/vector/share types), `Packet::write_labeled`/`write_many_labeled`/`composition`, and a `content_count` field on `SendData`/`ReceiveData`. Plus two reorgs: `net::simulation::runtime` → `simulator` and the real-TLS backend extracted to `net::tcp` (`TcpNetwork` re-exported from `net`). Breaking (module rename, `Packet` representation + private `Packet::new`, new `Event` fields, removed legacy `Event::HasData`), so a minor bump per §2. Added `examples/send_different_types.rs`. Tagged `v0.6.0`. |
| **0.7.0**       | _Feldman VSS hardening + property tests_ ✅ **RELEASED 2026-06-30** | New required `EllipticCurve::is_on_curve` method; `FeldmanSS::is_valid` rejects off-curve dealer commitments before `scalar_mul` (§6), surfaced as `ShareError::InvalidShare`; Tier-3 adversarial Feldman tests (off-curve commitment, tampered share, wrong commitment-vector length, length mismatch) and point-level on-curve regression tests. Plus testing-plan **Tier 2** (complete): a `proptest` suite (ring/field laws, Shamir subset-invariance, polynomial evaluate-then-interpolate recovery, `postcard` serialization round-trips for fields/curve points/`ShamirSS`/`FeldmanSS`/`Packet`) with shared strategies in `tests/common/mod.rs`. Tier-3 progress: `Packet` read/`pop` rejection tests (the 0.4.0 `Result` API), and `interpolate_polynomial_at` now returns `poly::Error::EmptyInterpolation`/`LengthMismatch` instead of panicking on malformed input (additive, `#[non_exhaustive]`). Breaking (new trait method), so a minor bump per §2. See `CHANGELOG.md` `[0.7.0]`. |
| **0.7.1**       | _Testing plan completion_ ✅ **RELEASED 2026-06-30** | Test- and documentation-only, no library API change (a patch per §2). Testing-plan **Tier 4** (generic `impl<E: Environment> Protocol<E>` migration of `tests/simulator.rs`, run-to-run reproducibility test, capability-carrying-environment test), **Tier 5** (real-network tests inline in `src/net/tcp.rs`: multi-party `recv_any` over mTLS, plus `ConnectionClosed`/`ConfigParse`/`InvalidPemFile` failure paths), and **Tier 6** per-constructor doctests on `Packet`/`ShamirSS`/`FeldmanSS`. The Tier 4 reordering harness and `cargo-deny` were declined. See `CHANGELOG.md` `[0.7.1]`. |
| **0.8.0**       | _MPC arithmetic layer_ ✅ **RELEASED 2026-07-06**  | The first §11 **Tier 1** slice: the **`LinearShare` trait** on the share types (local-op overloads, `encode_party` — Shamir maps party `i` to field point `i + 1`, keeping `0`-based network ids off the secret's point — trait-level deal/reconstruct; the reshaped `AdditiveSS` and `PartyId` derives are the batched breaking changes per §2) plus the **passive-adversary deal/open protocols** (`protocol::share`: `PassiveDealShr`, `PassiveOpenShr`, `PassiveOpenToParty`), the `protocol::Error::{Share, Input}` variants, and end-to-end simulator tests over both schemes (`tests/protocol_share.rs`). See `CHANGELOG.md` `[0.8.0]`. |
| **0.8.1**       | _Simulator FIFO fix_ ✅ **RELEASED 2026-07-06**    | Bug-fix patch per §2: the switchboard now keeps **per-link deliveries FIFO** — under the size-dependent delay model, a later-but-smaller message could overtake an earlier-but-larger one on the same link (impossible on a real TCP stream), which mispaired shares with senders and flakily broke Shamir reconstruction in the 0.8.0 open protocols (≈1/32 per run via `postcard` varint sizes). Arrival times are clamped monotone per link; regression test pins the ordering. See `CHANGELOG.md` `[0.8.1]`. |
| **0.8.2**       | _Virtual-time recv timeout_ ✅ **RELEASED 2026-07-07** | The §14 in-protocol timeout primitive and piece (1) of the §11.1 malicious-model receives: **`Network::recv_from_with_timeout(party, timeout)`**, returning the new `NetworkError::Timeout(PartyId)` that names the silent party. On the simulator the deadline is a **virtual-clock timer event scheduled on the switchboard** (captured on first poll as `recipient clock + timeout`; a packet arriving exactly at the deadline wins, matching `tokio::time::timeout`, to which `TcpNetwork` maps the call), so both backends keep identical semantics. Regression tests in `tests/recv_timeout.rs`. _Shipped as a required trait method in a patch — a deliberate one-time exception to §2 (breaking for external `Network` implementors)._ See `CHANGELOG.md` `[0.8.2]`. |
| **0.9.0**       | _recv-any timeout — timeout primitive complete_ ✅ **RELEASED 2026-07-08** | Completes the §14 timeout primitive: **`Network::recv_any_with_timeout(timeout)`** returns the next packet from *any* party together with its sender, or `NetworkError::Timeout(None)` at the deadline (no single culprit). Same switchboard virtual-clock timer design as 0.8.2 (one timer per call; the deadline is checked on the empty path, so an all-silent quorum times out instead of deadlocking the scheduler; post-deadline packets stay queued). Batched breaking changes per §2: `Timeout` carries `Option<PartyId>`, `recv_any` returns `(PartyId, Packet)`, and the new method is required on `Network`. Regression tests (all-silent / late-packet / prompt sender) in `tests/recv_timeout.rs`; module docs for the simulation internals. See `CHANGELOG.md` `[0.9.0]`. |
| **0.9.1**       | _recv-any timeout cleanup_ ✅ **RELEASED 2026-07-08** | Internal bug-fix patch per §2: `recv_any_with_timeout`'s timeout path now clears the wakers it parked on each inbound link, matching the success path (the leftovers were harmless — an extra ignored re-poll at worst — but the bookkeeping is now symmetric). No API change. See `CHANGELOG.md` `[0.9.1]`. |
| **0.10.0**      | _Hooks, protocol ids & bandwidth profiling_ ✅ **RELEASED 2026-07-10** | The `TriggeredHook` extension point extracted from `switchboard` into `net::simulation::hook`, plus the first built-in hook, **`MetricHook`** (totals the payload bytes each party sends, in aggregate and per sender; self-sends excluded). Plus **`Protocol::name` → `Protocol::id`**, returning the `Copy` `ProtocolId` newtype over `&'static str` (prelude-exported) that also types the `protocol_name` field on the `ProtocolBegin`/`ProtocolEnd` events. Both breaking, so a minor bump per §2. On top of these, **post-hoc bandwidth profiling**: `SimulationOutcome::bandwidth_tree_for` rebuilds the per-protocol call tree (`ProtocolBandwidthTree`, self bytes per node) from a party's trace, and `write_folded` exports it in the folded-stacks flamegraph format (§14), demoed in `examples/bandwidth_flamegraph.rs` and `examples/secure_stats_flamegraph.rs`. See `CHANGELOG.md` `[0.10.0]`. |
| **0.x**         | _Tier 1 remainder — "real MPC"_                    | The rest of §11 Tier 1: `open_many` (batched opening), error-detecting Shamir reconstruction, trusted-dealer **Beaver-triple `mul`** with a worked `examples/` circuit, and the active deal/open variants built on the 0.8.2/0.9.0 timeout receives. Turns the crate into "real MPC." |
| **0.x**         | _MPC protocol library (Tiers 2–4)_                 | The rest of §11 as it is scoped in: shared randomness / PRSS / coin-tossing, `recv_any`-based broadcast, shared linear algebra, commitments, comparison/bit-decomposition, and additional sharing schemes (replicated, packed). Sequenced by the §11.6 dependency graph. |
| **0.x**         | _Hardening & completeness_                         | Remaining §6 (constant-time review — deferred; threat-model doc) and chosen §10 features. _(The real-TLS deployment example landed in `real_tls_send_recv.rs`; `CONTRIBUTING.md` is deferred until there are outside contributors.)_                          |
| **0.x (stable)** | _API settled — steady state_                      | The §5 work has baked, public enums are `#[non_exhaustive]`, docs/examples are complete, and breaks are rare and deliberate. This is the intended steady state; `1.0` stays optional and unplanned (§2).                                                     |

---

## 13. Definition of a stable `0.x`

The bar for considering the `0.x` API "settled" — the steady state of §12, not a `1.0` gate:

- [x] License recorded in `Cargo.toml` and consistent with `LICENSE`. _(Done — AGPL-3.0-or-later.)_
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
- [x] `examples/` cover simulator + real deployment + secret sharing; `CHANGELOG.md` current.
      _(Done — `simple_send_recv.rs` (simulator), `additive_shr_secure_sum.rs` (secret sharing), and
      `real_tls_send_recv.rs` (real mTLS deployment) are all present, and `CHANGELOG.md` is current.)_
- [ ] `docs.rs` renders cleanly; README on-ramp is accurate end to end.

---

## 14. Deferred (later `0.x` or beyond)

- Constant-time / side-channel hardening (§6) — deliberately deferred while prototyping.
- `CONTRIBUTING.md` (§9) — deferred until the project attracts contributors beyond the sole
  maintainer; a contribution guide has no audience with a single author.
- Adversarial/reordering simulation harness (delay/drop/reorder deliveries) — a payoff of the
  explicit-blocking-state design (`Poll::Pending` = "party blocked on recv"). _(As a **test**
  harness this was declined 2026-06-30 (§10); it is kept here only as a possible future **simulator
  feature**.)_ If built, it lands as a `TriggeredHook` in `net::simulation::hook` (§7) — the trait
  already hands a hook `&mut Switchboard`, which is the steering seam it needs.
- Packet loss / retransmission modeling in the event loop.
- Compute-time / sender-side cost modeling in the virtual clock.
- **Flamegraph export of per-protocol bandwidth.** _(✅ **Shipped in 0.10.0** —
  `SimulationOutcome::bandwidth_tree_for` reconstructs the per-call-path tree from the
  `ProtocolBegin`/`ProtocolEnd`/`SendData` events in a party's trace, and
  `ProtocolBandwidthTree::write_folded(&mut impl io::Write)` serializes it; demoed end to end in
  `examples/bandwidth_flamegraph.rs`. Idea recorded 2026-07-09; scoped down the same day from an
  in-terminal renderer — scl-rs renders nothing.)_ The export uses the standard **folded-stacks
  format** of Brendan Gregg's flamegraph tooling
  (<https://www.brendangregg.com/flamegraphs.html>): one line per call path, frames
  semicolon-separated root-first, then the value —
  `<simulation>;SecSumShamirShr;InputPhase;PassiveDealLinearShr 20`. The user renders it with the
  tool of their preference (`flamegraph.pl`, `inferno`, speedscope), so scl-rs takes on **zero
  rendering dependencies** and gets interactive SVG for free. Each line's value is the node's
  **self bytes**, not inclusive — flamegraph tools sum children into parents themselves, so
  emitting inclusive values would double-count. **Remaining sibling feature:** bytes is one of two
  natural axes — events carry virtual-time timestamps, so a time-in-protocol-scope export could
  share the same tree reconstruction and the same folded format.
- **In-protocol timeout / deadline primitive (virtual-time).** _(✅ **Complete** — 0.8.2 shipped
  `Network::recv_from_with_timeout` and 0.9.0 `Network::recv_any_with_timeout`, exactly per this
  design; see §11.1.)_ Let a protocol wait on a `Network` operation *with a deadline* and
  proceed if nothing arrives in time — needed by partially-synchronous protocols (round timeouts,
  BFT view-change timers). It cannot be built on `tokio::time::timeout`, whose clock is wall-clock:
  under the deterministic simulator the deadline must be a **virtual-time** event scheduled on the
  switchboard that fires at a virtual instant and wakes the parked party, so a timeout and a
  message race *deterministically*. Exposed through the `Network`/`Environment` API so one code
  path runs on both backends (mapping to `tokio::time::timeout` on a real deployment). Was the only
  protocol-facing capability with no executor-agnostic workaround — surfaced during the
  `send_many`/concurrency design.
- Broader MPC protocol library on top of the typed-composition core. **Now a concrete, tiered plan
  in §11** (`LinearShare` arithmetic layer, opening/Beaver multiplication, shared
  randomness/broadcast, linear algebra/comparison/commitments, and additional sharing schemes),
  with a proposed `0.8.0` Tier-1 slice (`LinearShare` + passive deal/open) in the §12 release
  sequence. OT/OLE, garbled circuits, and malicious-security extensions are the parts still
  deferred (see §11.5).
