# scl-rs Roadmap to v1.0

**Date:** 2026-06-11
**Current version:** 0.1.0 (unpublished)
**Goal:** ship a first crates.io release that external users can depend on, then reach a
semver-stable **v1.0**.

This document is a living plan. It captures where the library is today, the decisions that gate a
public release, the work grouped into themed workstreams, a suggested version sequence, and an
explicit **v1.0 Definition of Done**.

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

**What's missing for a release** is mostly _productization_, not core features: crates.io metadata,
a deliberate license posture, a security disclaimer, API stabilization, and hardening. Those are the
body of this roadmap.

---

## 2. What "v1.0" commits us to

Publishing 1.0 is a **semver promise**: the public API won't break until 2.0. Everything users can
name — traits (`Ring`, `FiniteField`, `Network`, `Protocol`), types (`Packet`, `Environment`,
`SimulationOutcome`, every `pub enum` error), function signatures, and the trait method receivers —
becomes a contract. The expensive-to-change items (§5: API stabilization) must therefore land
**before** 1.0, while breaking them is still free. Anything we're unsure about should ship in a
`0.x` first and bake.

---

## 3. Two gating decisions (resolve first — they color everything else)

### D-A. License posture

The repo ships **AGPL‑3.0** (`LICENSE`), but `Cargo.toml` has **no `license` field at all** — which
is a hard crates.io publish blocker. Beyond the mechanical fix, AGPL is the most restrictive common
OSS license: it obliges anyone who _uses the library over a network_ to release their source. For a
library whose entire purpose is to be _consumed by others_, that will deter most downstream adoption
(companies routinely ban AGPL dependencies).

**Action:** make a deliberate choice and record it.

- If broad adoption is the goal, the Rust-ecosystem norm is **dual `MIT OR Apache‑2.0`**.
- If copyleft is intentional, keep AGPL but understand and document the consequence.
- Either way, set `license = "<SPDX>"` in `Cargo.toml` to match the `LICENSE` file(s).

This is the author's call; the roadmap only flags that it must be _decided_, not defaulted.

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

Mechanical, do first; unblocks an early `0.2` on crates.io.

- [ ] Add `license` (or `license-file`) to `Cargo.toml` — **publish blocker** (see D-A).
- [ ] Add `description` — required-quality metadata; crates.io surfaces it.
- [ ] Add `documentation = "https://docs.rs/scl-rs"`, `homepage`, confirm `repository`, `readme`,
      `keywords`, `categories`.
- [ ] Declare an **MSRV**: `rust-version = "1.XX"` and test it in CI.
- [ ] Narrow tokio features: `tokio = { features = ["full"] }` pulls everything into every downstream
      build. Scope to what's used (likely `rt`, `rt-multi-thread`, `net`, `io-util`, `time`, `sync`,
      `macros`).
- [ ] `cargo publish --dry-run` clean; verify the packaged file list (`exclude`/`include` for
      `certs/`, scratch files, large docs if desired).
- [ ] Verify docs.rs build (it builds all features on a fixed toolchain).

## 5. Workstream — API stabilization (the heart of v1.0; breaking now, frozen later)

Each item below is a breaking change that is cheap today and expensive after 1.0.

- [ ] **`Packet` consumer API is error-swallowing.** `read(idx) -> Option<T>` and `pop() -> Option<T>`
      silently return `None` on a deserialize failure or wrong index, while `write` returns `Result`.
      Move reads to `Result<T, _>` with a real error so consumers can distinguish "absent" from
      "malformed." (`src/net/mod.rs`.)
- [ ] **Error-type consistency sweep.** The crate exposes many independent error enums
      (`ChannelError`, `NetworkError`, `SimulationError`, `ShareError`, `protocol::Error`,
      `poly::Error`, `matrix` errors). Review naming, `#[non_exhaustive]` on public enums (lets you
      add variants post-1.0 without breaking), and whether `NetworkConfig::new` should return a crate
      error instead of leaking `std::io::Result`.
- [ ] **`Protocol` receiver decision.** Settle `&self` vs consuming `self` (the latter lets a protocol
      move non-`Clone` inputs into `run`). Changing the receiver is breaking — decide before 1.0.
- [ ] **`Environment::clock()` is a vestigial wall clock** — it reports real elapsed time, which is
      meaningless under the deterministic simulator. Either wire it to virtual time
      (`Switchboard::clock_of`) or remove it before it becomes a 1.0 commitment. (`src/protocol.rs`.)
- [ ] **`simulate<P>` ergonomics.** All parties must share one concrete `P`; confirm that's the
      intended public contract (role asymmetry via branching on the local party) and document it.
- [ ] **Re-exports / prelude.** Only `Protocol` is re-exported at the crate root. Add a small
      `prelude` (or curated root re-exports) for the common path (`Network`, `Packet`, `PartyId`,
      `Environment`, `simulate`, the field/ring traits) so users aren't deep-pathing.
- [ ] **Naming/visibility audit.** Walk every `pub` item; demote internals to `pub(crate)`; fix
      inconsistencies (e.g. the `ss::ec` module's outer doc uses `//` not `///`).

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

- [ ] **`channel.rs::send` is missing `self.flush().await?`** after its writes — `tokio-rustls`
      buffers ciphertext, so a strict request→response over real TLS can stall under backpressure.
      _(Highest-priority correctness fix.)_
- [ ] **No real-TLS integration test.** The simulator suite never touches `TcpNetwork`. Add a
      localhost two-task `#[tokio::test]` covering handshake + length-prefixed framing + flush +
      close end-to-end.
- [ ] **Trace `channel_id` perspective bug.** `Switchboard::send`/`try_recv` record events with the
      canonical `link.channel_id()` (min→max) instead of `ChannelId::new(recorder, peer)`, so
      `Event::Display` renders arrows backwards for the higher-id party.
- [ ] **Nested protocol calls are invisible in traces** — only the top-level protocol records
      `ProtocolBegin`/`End`. Decide whether nested `.await` calls should appear (needs a recording
      hook reachable from the network-generic `Environment`).
- [ ] **D10 unification.** Collapse the duplicated `Link {recipient,sender}` and
      `ChannelId {local,remote}` + `flip_end_points` into one directed pair type; re-key
      `NetworkConfig::channel_config` to `Link` (also enables asymmetric up/down links).

## 8. Workstream — Quality gates & CI

Today CI only runs `check` / `build` / `test`. Harden it so releases are mechanical:

- [ ] `cargo fmt --check`.
- [ ] `cargo clippy --all-targets -- -D warnings` (clears the ~15 pre-existing style lints:
      `Clock` `Default`, `module_inception` on `tests/simulator/mod.rs`, `needless_borrow`, …).
- [ ] `cargo doc --no-deps -D warnings` in CI (keep intra-doc links honest).
- [ ] `cargo test` across the MSRV and stable; consider a matrix (Linux at minimum).
- [ ] `cargo publish --dry-run` on tags.
- [ ] Optional: coverage reporting; `cargo-audit`/`deny` (see §6).

## 9. Workstream — Docs, examples & ecosystem

- [ ] **`examples/` directory** (none today). At minimum: (a) a simulator run, (b) a real two-party
      TLS deployment (the binary sketched in the crate docs), (c) a secret-sharing round-trip.
      Runnable examples are the fastest on-ramp for new users.
- [ ] **`CHANGELOG.md`** (Keep a Changelog format) from 0.1.0 onward.
- [ ] **`CONTRIBUTING.md`** and **`SECURITY.md`** (disclosure policy + the §3 disclaimer).
- [ ] Refresh `README.md`'s "Missing features" into a link to this roadmap; keep the security banner
      at the top.
- [ ] Optional rename `runtime.rs` → `simulator.rs` (cosmetic; do before 1.0 if at all — module paths
      are public).

## 10. Workstream — Feature completeness (scope to taste)

These are not strictly required for 1.0 but shape how "complete" the first stable release feels.

- [ ] **Arbitrary prime-`p` field** (open README item): a general `F_p` instead of only the
      hand-written Mersenne‑61 / secp256k1 fields.
- [ ] **Test-coverage gap** (open README item): "write missing tests for all functionalities" —
      especially `net` (real path), `matrix`/`poly` edge cases, and serialization round-trips.
- [ ] Any additional MPC facilities you want in the 1.0 surface (e.g. opening/reconstruction helpers,
      a Beaver-triple/multiplication example to showcase typed composition end-to-end).

---

## 11. Suggested release sequence

Ship early and often on `0.x`; let the API bake before locking it at 1.0.

| Version         | Theme                      | Contents                                                                                                                                                |
| --------------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **0.2.0**       | _Publishable & honest_     | §4 (metadata, license field, tokio features), the §3 security disclaimer + `SECURITY.md`, the §7 `flush` fix. First crates.io release, clearly pre-1.0. |
| **0.3.0**       | _Correct & clean_          | Remaining §7 loose ends (real-TLS test, `channel_id` bug), §8 CI hardening (clippy/fmt/doc gates green).                                                |
| **0.4.0 → 0.x** | _API stabilization_        | §5 in full (Packet `Result` API, error sweep, `Protocol` receiver, `Environment` clock, prelude, naming audit). Each is breaking — batch and document.  |
| **0.x**         | _Hardening & completeness_ | §6 (CSPRNG bounds, constant-time review, cargo-audit), §9 examples/docs, chosen §10 features.                                                           |
| **1.0.0**       | _Stabilize & release_      | Freeze the API, finalize docs/examples, lock the license + threat-model statements, `cargo publish --dry-run` clean, tag and publish.                   |

---

## 12. v1.0 — Definition of Done

- [ ] License decided, recorded in `Cargo.toml`, and consistent with `LICENSE`.
- [ ] Security posture + threat model documented; `SECURITY.md` present; disclaimer prominent.
- [ ] Public API reviewed and deliberately frozen: `Packet` reads return `Result`; public error enums
      `#[non_exhaustive]`; `Protocol` receiver and `Environment::clock` settled; prelude in place.
- [ ] Secret-generation APIs require a CSPRNG (or the limitation is documented as a conscious choice).
- [ ] All §7 correctness loose ends closed (notably the TLS `flush` and a real-TLS integration test).
- [ ] CI green on fmt, `clippy -D warnings`, `doc -D warnings`, tests across MSRV+stable,
      `publish --dry-run`.
- [ ] `examples/` cover simulator + real deployment + secret sharing; `CHANGELOG.md` current.
- [ ] `docs.rs` renders cleanly; README on-ramp is accurate end to end.

---

## 13. Deferred to post-1.0

- Nested-call trace visibility (§7) if not done by 1.0.
- Adversarial/reordering simulation harness (delay/drop/reorder deliveries) — a payoff of the
  explicit-blocking-state design (`Poll::Pending` = "party blocked on recv").
- Packet loss / retransmission modeling in the event loop.
- Compute-time / sender-side cost modeling in the virtual clock.
- Broader MPC protocol library on top of the typed-composition core.
